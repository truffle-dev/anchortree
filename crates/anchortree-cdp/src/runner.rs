//! Phase 3.3b: wire the browser-free [`HarRecorder`] to a live CDP event stream
//! and emit the WebArena-Verified agent contract output for one task.
//!
//! 3.3a built the recorder as a pure state machine with no browser in it. This
//! module is the thin live layer on top: [`NetworkCapture`] subscribes to the
//! four `Network.*` event streams off a local [`chromiumoxide::Page`], pumps
//! every event into a recorder on a background task, and hands back the finished
//! [`Har`] when the caller is done driving the page. [`write_task_output`] then
//! writes the two files the WebArena-Verified runner consumes for a task:
//! `agent_response.json` and `network.har`.
//!
//! ## Why the local `Page`, not the thin channel
//!
//! The hosted [`RawCdpSession`](crate::channel::RawCdpSession) read loop drains
//! and discards CDP events, so it is not an event sink (DECISIONS D26). Live HAR
//! capture therefore rides the local `chromiumoxide::Page` path —
//! `Page::event_listener::<T>()` yields an `EventStream<T>` of `Arc<T>` that is
//! the real event tap. A hosted/OOPIF HAR capture would have to surface events
//! out of the channel read loop and is a separate, later item.
//!
//! ## Shape, designed for an agent driving a browser
//!
//! ```ignore
//! let capture = NetworkCapture::start(page).await?; // Network.enable + subscribe
//! page.goto("https://example.test/task").await?;    // do the task
//! // ... observe, act, read the answer out of the DOM ...
//! let har = capture.finish().await?;                 // stop + drain + build HAR
//! write_task_output(out_dir, &AgentResponse::retrieved(answer), &har)?;
//! ```
//!
//! The capture runs concurrently with whatever browser work the caller does
//! between `start` and `finish`; the caller never has to hand its work to a
//! closure. `finish` signals the pump to stop, drains any events already queued
//! in the channel, and returns the assembled HAR.

use std::path::Path;
use std::sync::Arc;
use std::{fmt, io};

use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::network::{
    EventLoadingFailed, EventLoadingFinished, EventRequestWillBeSent, EventResponseReceived,
};
use futures::stream::{self, BoxStream};
use futures::{FutureExt as _, StreamExt as _};
use serde::Serialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::CdpError;
use crate::har::{self, Har, HarRecorder};

/// One merged network event, tagged by which of the four CDP streams produced
/// it, so the pump can fold it into the right [`HarRecorder`] entry point.
enum NetEvent {
    Will(Arc<EventRequestWillBeSent>),
    Resp(Arc<EventResponseReceived>),
    Fin(Arc<EventLoadingFinished>),
    Fail(Arc<EventLoadingFailed>),
}

impl NetEvent {
    /// Dispatch this event to the matching recorder folding method.
    fn record_into(&self, rec: &mut HarRecorder) {
        match self {
            NetEvent::Will(e) => rec.on_request_will_be_sent(e),
            NetEvent::Resp(e) => rec.on_response_received(e),
            NetEvent::Fin(e) => rec.on_loading_finished(e),
            NetEvent::Fail(e) => rec.on_loading_failed(e),
        }
    }
}

/// Either a folded network event or the stop signal, unified so the pump can
/// consume one `select`ed stream instead of racing a stream against a future
/// (which would need the `tokio`/`futures` `select!` machinery the library does
/// not pull in).
enum Control {
    Event(NetEvent),
    Stop,
}

/// A live network capture in flight against a local [`Page`].
///
/// Created by [`start`](NetworkCapture::start), closed by
/// [`finish`](NetworkCapture::finish). Between the two, drive the page however
/// the task requires; the four `Network.*` streams are pumped into a
/// [`HarRecorder`] on a background task the whole time.
pub struct NetworkCapture {
    pump: JoinHandle<HarRecorder>,
    stop: oneshot::Sender<()>,
}

impl fmt::Debug for NetworkCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkCapture").finish_non_exhaustive()
    }
}

impl NetworkCapture {
    /// Enable `Network` tracking and start pumping its events into a recorder.
    ///
    /// Subscribes one [`EventStream`](chromiumoxide) per Network event type,
    /// merges the four into a single stream, and spawns a background task that
    /// folds each event into a [`HarRecorder`] until [`finish`] is called.
    ///
    /// Must be called from within a Tokio runtime (the pump is a spawned task).
    /// The page's CDP handler must be driven for events to flow — a [`Session`]
    /// from [`connect`](crate::connect) already does this.
    ///
    /// [`finish`]: NetworkCapture::finish
    /// [`Session`]: crate::Session
    pub async fn start(page: &Page) -> Result<Self, CdpError> {
        // Subscribe BEFORE enabling so no early request can slip between the
        // `Network.enable` ack and the listeners being installed.
        let wills = page
            .event_listener::<EventRequestWillBeSent>()
            .await?
            .map(NetEvent::Will);
        let resps = page
            .event_listener::<EventResponseReceived>()
            .await?
            .map(NetEvent::Resp);
        let fins = page
            .event_listener::<EventLoadingFinished>()
            .await?
            .map(NetEvent::Fin);
        let fails = page
            .event_listener::<EventLoadingFailed>()
            .await?
            .map(NetEvent::Fail);

        har::enable(page, None).await?;

        let events: BoxStream<'static, NetEvent> =
            stream::select(stream::select(wills, resps), stream::select(fins, fails)).boxed();

        let (stop_tx, stop_rx) = oneshot::channel();
        let pump = tokio::spawn(pump(events, stop_rx));
        Ok(Self {
            pump,
            stop: stop_tx,
        })
    }

    /// Stop the capture and return the assembled [`Har`].
    ///
    /// Signals the pump to stop, lets it drain any events already queued in the
    /// channel (so a `loadingFinished` that landed just before the stop still
    /// counts), and finalizes the HAR. Requests still in flight at stop time are
    /// emitted as entries with `time = -1`, in start order.
    pub async fn finish(self) -> Result<Har, CdpError> {
        // If the pump already exited (its streams closed), the receiver is gone
        // and this send is a no-op; either way we then await the recorder.
        let _ = self.stop.send(());
        let recorder = self
            .pump
            .await
            .map_err(|e| CdpError::Malformed(format!("network capture pump task failed: {e}")))?;
        Ok(recorder.into_har())
    }
}

/// Background task: fold events into a recorder until stopped, then drain.
async fn pump(events: BoxStream<'static, NetEvent>, stop: oneshot::Receiver<()>) -> HarRecorder {
    let mut recorder = HarRecorder::new();

    // Fold the stop signal into the same stream as the events so the loop reads
    // one source. `stop` yields exactly once (when `finish` fires or the sender
    // drops); after that the events stream carries the loop.
    let stop_stream = stream::once(async move {
        let _ = stop.await;
    })
    .map(|()| Control::Stop)
    .boxed();
    let mut combined = stream::select(events.map(Control::Event), stop_stream);

    while let Some(control) = combined.next().await {
        match control {
            Control::Event(ev) => ev.record_into(&mut recorder),
            Control::Stop => {
                // Drain whatever is already buffered without awaiting new
                // arrivals, then finish. `now_or_never` polls the next-future
                // once: `Some(Some(c))` is a ready item, `Some(None)` is a
                // closed stream, `None` is "would block" — all three stop us.
                while let Some(Some(Control::Event(ev))) = combined.next().now_or_never() {
                    ev.record_into(&mut recorder);
                }
                break;
            }
        }
    }

    recorder
}

/// The kind of WebArena-Verified task, serialized to the runner's screaming-case
/// vocabulary (`RETRIEVE` / `MUTATE` / `NAVIGATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskType {
    /// Read a value out of the page (the answer lands in `retrieved_data`).
    Retrieve,
    /// Change server state (form submit, cart add, setting toggle).
    Mutate,
    /// Reach a target URL/state.
    Navigate,
}

/// The outcome an agent reports for a task.
///
/// Only the values verified against the runner contract (DECISIONS D26) are
/// modeled here — `SUCCESS` plus the two named error terminals. The full error
/// vocabulary should be pinned against the runner before 3.3d's multi-task loop;
/// the first 3.3b target is a single RETRIEVE task that reports `SUCCESS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    /// The task completed and any required data was produced.
    Success,
    /// The requested item/answer was not present.
    NotFoundError,
    /// The action was blocked by the site's permissions.
    PermissionDeniedError,
}

/// The `agent_response.json` payload the WebArena-Verified runner reads per task.
///
/// All four keys are always emitted (the absent optionals serialize as `null`)
/// because the runner reads the object by fixed key.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    /// What kind of task this was.
    pub task_type: TaskType,
    /// How it ended.
    pub status: TaskStatus,
    /// The answer payload for a RETRIEVE task; `null` otherwise.
    pub retrieved_data: Option<serde_json::Value>,
    /// A human-readable failure note for an error status; `null` on success.
    pub error_details: Option<String>,
}

impl AgentResponse {
    /// A successful RETRIEVE carrying its answer payload.
    pub fn retrieved(data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::Retrieve,
            status: TaskStatus::Success,
            retrieved_data: Some(data),
            error_details: None,
        }
    }

    /// A successful MUTATE or NAVIGATE that produces no read-back data.
    pub fn completed(task_type: TaskType) -> Self {
        Self {
            task_type,
            status: TaskStatus::Success,
            retrieved_data: None,
            error_details: None,
        }
    }

    /// A failed task: a non-success status with a diagnostic note.
    pub fn failed(task_type: TaskType, status: TaskStatus, details: impl Into<String>) -> Self {
        Self {
            task_type,
            status,
            retrieved_data: None,
            error_details: Some(details.into()),
        }
    }

    /// Serialize to the exact pretty JSON written to `agent_response.json`.
    pub fn to_json(&self) -> String {
        // The derived `Serialize` cannot fail for these owned, finite fields.
        serde_json::to_string_pretty(self).expect("AgentResponse is always serializable")
    }
}

/// Write the two files the WebArena-Verified runner consumes for one task into
/// `output_dir`: `agent_response.json` and `network.har` (exact filenames).
///
/// Creates `output_dir` if it does not exist. The HAR filename is fixed by the
/// runner contract; do not rename it.
pub fn write_task_output(output_dir: &Path, response: &AgentResponse, har: &Har) -> io::Result<()> {
    std::fs::create_dir_all(output_dir)?;
    std::fs::write(
        output_dir.join("agent_response.json"),
        response.to_json().as_bytes(),
    )?;
    std::fs::write(output_dir.join("network.har"), har.to_json().as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieved_response_serializes_with_screaming_enum_values() {
        let resp = AgentResponse::retrieved(serde_json::json!("$1,299.00"));
        let v: serde_json::Value = serde_json::from_str(&resp.to_json()).unwrap();
        assert_eq!(v["task_type"], "RETRIEVE");
        assert_eq!(v["status"], "SUCCESS");
        assert_eq!(v["retrieved_data"], "$1,299.00");
        // The error key is present and null, not omitted: the runner reads by
        // fixed key.
        assert!(v.get("error_details").is_some());
        assert!(v["error_details"].is_null());
    }

    #[test]
    fn failed_response_serializes_error_terminal() {
        let resp = AgentResponse::failed(
            TaskType::Retrieve,
            TaskStatus::NotFoundError,
            "no product matched the SKU",
        );
        let v: serde_json::Value = serde_json::from_str(&resp.to_json()).unwrap();
        assert_eq!(v["status"], "NOT_FOUND_ERROR");
        assert_eq!(v["error_details"], "no product matched the SKU");
        assert!(v["retrieved_data"].is_null());
    }

    #[test]
    fn completed_mutate_has_no_data_and_no_error() {
        let resp = AgentResponse::completed(TaskType::Mutate);
        let v: serde_json::Value = serde_json::from_str(&resp.to_json()).unwrap();
        assert_eq!(v["task_type"], "MUTATE");
        assert_eq!(v["status"], "SUCCESS");
        assert!(v["retrieved_data"].is_null());
        assert!(v["error_details"].is_null());
    }

    #[test]
    fn write_task_output_emits_both_files_with_exact_names() {
        // Dependency-free unique temp dir: pid + a monotonic-ish nanos suffix.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "anchortree-runner-test-{}-{}",
            std::process::id(),
            nanos
        ));

        let response = AgentResponse::retrieved(serde_json::json!({"price": 1299}));
        let har = HarRecorder::new().into_har();
        write_task_output(&dir, &response, &har).unwrap();

        let resp_path = dir.join("agent_response.json");
        let har_path = dir.join("network.har");
        assert!(resp_path.exists(), "agent_response.json must be written");
        assert!(har_path.exists(), "network.har must be written");

        // Both files are valid JSON, and the HAR round-trips back to a 1.2 log.
        let resp_back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&resp_path).unwrap()).unwrap();
        assert_eq!(resp_back["task_type"], "RETRIEVE");
        let har_back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&har_path).unwrap()).unwrap();
        assert_eq!(har_back["log"]["version"], "1.2");

        // Clean up so /tmp does not accumulate across test runs.
        let _ = std::fs::remove_dir_all(&dir);
    }
}
