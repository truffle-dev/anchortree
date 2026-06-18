//! Phase 3.3b: wire the browser-free [`HarRecorder`] to a live CDP event stream
//! and emit the WebArena-Verified agent contract output for one task.
//!
//! 3.3a built the recorder as a pure state machine with no browser in it. This
//! module is the thin live layer on top: [`NetworkCapture`] subscribes to the
//! five `Network.*` event streams off a local [`chromiumoxide::Page`], pumps
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
//!
//! ## Body capture: making the recording self-contained
//!
//! [`start`](NetworkCapture::start) records the network trace — URLs, methods,
//! status, headers, timings — which is all the WebArena-Verified
//! `NetworkEventEvaluator` scores from. It does **not** capture response bodies,
//! so its HAR cannot be replayed offline. [`start_with_bodies`] does: when a
//! request's `loadingFinished` event arrives the pump issues
//! `Network.getResponseBody` for it and feeds the bytes into the recorder
//! *before* the entry finalizes, producing a SELF-CONTAINED inline-body HAR (the
//! input the [`ReplayFulfiller`](crate::ReplayFulfiller) replays with no live
//! origin — DECISIONS D34 step b). The body read is best-effort: a request whose
//! body is unavailable (a redirect hop, an evicted cache entry) finalizes
//! body-less rather than aborting the capture. The extra CDP round-trip per
//! request is why it is a separate, opt-in constructor and not the default.
//!
//! [`start_with_bodies`]: NetworkCapture::start_with_bodies

use std::path::Path;
use std::sync::Arc;
use std::{fmt, io};

use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::network::{
    EventLoadingFailed, EventLoadingFinished, EventRequestWillBeSent,
    EventRequestWillBeSentExtraInfo, EventResponseReceived, GetResponseBodyParams,
};
use futures::stream::{self, BoxStream};
use futures::{FutureExt as _, StreamExt as _};
use serde::Serialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::CdpError;
use crate::har::{self, Har, HarRecorder, ResponseBody};

/// One merged network event, tagged by which of the five CDP streams produced
/// it, so the pump can fold it into the right [`HarRecorder`] entry point.
enum NetEvent {
    Will(Arc<EventRequestWillBeSent>),
    WillExtra(Arc<EventRequestWillBeSentExtraInfo>),
    Resp(Arc<EventResponseReceived>),
    Fin(Arc<EventLoadingFinished>),
    Fail(Arc<EventLoadingFailed>),
}

impl NetEvent {
    /// Dispatch this event to the matching recorder folding method.
    fn record_into(&self, rec: &mut HarRecorder) {
        match self {
            NetEvent::Will(e) => rec.on_request_will_be_sent(e),
            NetEvent::WillExtra(e) => rec.on_request_will_be_sent_extra_info(e),
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
/// the task requires; the five `Network.*` streams are pumped into a
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
    /// merges the five into a single stream, and spawns a background task that
    /// folds each event into a [`HarRecorder`] until [`finish`] is called.
    ///
    /// Must be called from within a Tokio runtime (the pump is a spawned task).
    /// The page's CDP handler must be driven for events to flow — a [`Session`]
    /// from [`connect`](crate::connect) already does this.
    ///
    /// [`finish`]: NetworkCapture::finish
    /// [`Session`]: crate::Session
    pub async fn start(page: &Page) -> Result<Self, CdpError> {
        Self::start_inner(page, false).await
    }

    /// Like [`start`](NetworkCapture::start), but also capture response bodies so
    /// the recording is self-contained and replayable offline.
    ///
    /// At each request's `loadingFinished` the pump issues
    /// `Network.getResponseBody` and feeds the bytes to the recorder before the
    /// entry finalizes, inlining the body into the HAR. Best-effort per request:
    /// an unavailable body (redirect, eviction) finalizes body-less. Costs one
    /// extra CDP round-trip per request, so reach for it when you intend to
    /// replay the HAR (DECISIONS D34 step b), not for a plain network trace.
    pub async fn start_with_bodies(page: &Page) -> Result<Self, CdpError> {
        Self::start_inner(page, true).await
    }

    /// Shared setup for both constructors: subscribe to the five `Network.*`
    /// streams, enable tracking, and spawn the pump. When `capture_bodies` is
    /// set the pump is handed an owned [`Page`] clone so it can read bodies.
    async fn start_inner(page: &Page, capture_bodies: bool) -> Result<Self, CdpError> {
        // Subscribe BEFORE enabling so no early request can slip between the
        // `Network.enable` ack and the listeners being installed.
        let wills = page
            .event_listener::<EventRequestWillBeSent>()
            .await?
            .map(NetEvent::Will);
        let will_extras = page
            .event_listener::<EventRequestWillBeSentExtraInfo>()
            .await?
            .map(NetEvent::WillExtra);
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

        let events: BoxStream<'static, NetEvent> = stream::select(
            stream::select(stream::select(wills, will_extras), resps),
            stream::select(fins, fails),
        )
        .boxed();

        // The pump needs an owned `Page` (Arc-backed clone) only when it will
        // read bodies; the plain trace path keeps the pump page-free.
        let body_page = capture_bodies.then(|| page.clone());

        let (stop_tx, stop_rx) = oneshot::channel();
        let pump = tokio::spawn(pump(events, stop_rx, body_page));
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
///
/// When `body_page` is `Some`, the pump reads each response's body at
/// `loadingFinished` time and feeds it to the recorder before the entry
/// finalizes (see [`record_event`]).
async fn pump(
    events: BoxStream<'static, NetEvent>,
    stop: oneshot::Receiver<()>,
    body_page: Option<Page>,
) -> HarRecorder {
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
            Control::Event(ev) => record_event(&mut recorder, ev, body_page.as_ref()).await,
            Control::Stop => {
                // Drain whatever is already buffered without awaiting new
                // arrivals, then finish. `now_or_never` polls the next-future
                // once: `Some(Some(c))` is a ready item, `Some(None)` is a
                // closed stream, `None` is "would block" — all three stop us.
                while let Some(Some(Control::Event(ev))) = combined.next().now_or_never() {
                    record_event(&mut recorder, ev, body_page.as_ref()).await;
                }
                break;
            }
        }
    }

    recorder
}

/// Fold one network event into the recorder.
///
/// For a `loadingFinished` event when body capture is on, first read the
/// response body over CDP (`Network.getResponseBody`) and feed it to the
/// recorder, so the entry the next line finalizes carries its bytes inline. The
/// read is best-effort: a request whose body is unavailable (a redirect hop, an
/// evicted entry) is left body-less rather than aborting the whole capture. The
/// body MUST be fed before `record_into` finalizes the entry, because
/// `on_loading_finished` removes the pending request — hence the read happens
/// here, ahead of the fold.
async fn record_event(rec: &mut HarRecorder, ev: NetEvent, body_page: Option<&Page>) {
    if let (NetEvent::Fin(fin), Some(page)) = (&ev, body_page)
        && let Ok(resp) = page
            .execute(GetResponseBodyParams::new(fin.request_id.clone()))
            .await
    {
        rec.on_response_body(
            fin.request_id.inner(),
            ResponseBody {
                text: resp.result.body.clone(),
                base64: resp.result.base64_encoded,
            },
        );
    }
    ev.record_into(rec);
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
/// This is the full closed set of six values the WebArena-Verified runner
/// accepts (DECISIONS D27, pinned against the runner's `status` vocabulary):
/// `SUCCESS` plus five error terminals. `UNKNOWN_ERROR` is the catch-all an
/// agent reports when no more specific terminal applies; use [`TaskStatus::unknown`]
/// at call sites that cannot classify a failure further.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    /// The task completed and any required data was produced.
    Success,
    /// The requested action is not permitted by the task's rules.
    ActionNotAllowedError,
    /// The action was blocked by the site's permissions.
    PermissionDeniedError,
    /// The requested item/answer was not present.
    NotFoundError,
    /// The produced data failed the task's validation contract.
    DataValidationError,
    /// A failure that does not fit any more specific terminal (catch-all).
    UnknownError,
}

impl TaskStatus {
    /// The catch-all error terminal, for failures that cannot be classified
    /// into a more specific status.
    pub fn unknown() -> Self {
        Self::UnknownError
    }
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
    fn all_six_task_statuses_serialize_to_exact_wire_spellings() {
        // The runner accepts exactly these six values; the wire spelling is the
        // contract, so pin every one against its SCREAMING_SNAKE_CASE string.
        let cases = [
            (TaskStatus::Success, "SUCCESS"),
            (
                TaskStatus::ActionNotAllowedError,
                "ACTION_NOT_ALLOWED_ERROR",
            ),
            (TaskStatus::PermissionDeniedError, "PERMISSION_DENIED_ERROR"),
            (TaskStatus::NotFoundError, "NOT_FOUND_ERROR"),
            (TaskStatus::DataValidationError, "DATA_VALIDATION_ERROR"),
            (TaskStatus::UnknownError, "UNKNOWN_ERROR"),
        ];
        for (status, wire) in cases {
            let v = serde_json::to_value(status).unwrap();
            assert_eq!(v, wire, "{status:?} must serialize to {wire}");
        }
        assert_eq!(TaskStatus::unknown(), TaskStatus::UnknownError);
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
