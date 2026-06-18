//! Phase 3.3b (iii) proof: get the first real WebArena-Verified `score` back out
//! of an offline replay, with no browser and no Docker site.
//!
//! This closes the runner loop. It writes the two files the runner consumes for
//! one RETRIEVE task — `agent_response.json` (a correct answer for the pinned
//! task) and a minimal one-entry `network.har` — into the `{root}/{task_id}`
//! layout, then drives `webarena-verified eval-tasks` over that directory and
//! parses the `eval_result.json` it writes back. For the pinned task the score
//! is `1.0`.
//!
//! The HAR is hand-built here rather than captured live: 3.3b (i)+(ii) already
//! proved the live pump (see `webarena_capture.rs`), and the `AgentResponseEvaluator`
//! that scores a RETRIEVE task never inspects HAR contents — the file only has to
//! parse with at least one entry (DECISIONS D27). So the whole eval runs offline
//! against a synthetic HAR; the HAR is the environment, there is no live site.
//!
//! ## CLI-gated, not browser-gated
//!
//! If the `webarena-verified` binary is not on `PATH` the example prints how to
//! install it and exits 0, so it never reddens CI (where the Python tool is
//! absent). With the tool installed it asserts the real score.
//!
//! ```text
//! # in a venv with `pip install webarena-verified`
//! cargo run -p anchortree-cdp --example eval_task
//! ```

use std::error::Error;

use anchortree_cdp::{
    AgentResponse, EvalError, Har, HarCache, HarContent, HarCookie, HarCreator, HarEntry,
    HarHeader, HarLog, HarQuery, HarRequest, HarResponse, HarTimings, run_eval_tasks,
    task_output_dir, write_task_output,
};

/// The pinned RETRIEVE-shopping task: "name(s) of reviewer(s) who mention ear
/// cups being small". Its expected answer is the four reviewer names below;
/// `AgentResponseEvaluator` matches unordered and case-insensitively.
const PINNED_TASK_ID: u32 = 21;

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!("anchortree-eval-task-{}", std::process::id()));
    let task_dir = task_output_dir(&root, PINNED_TASK_ID);

    // (ii) the agent response: the correct, complete answer for task 21.
    let response = AgentResponse::retrieved(serde_json::json!([
        "Catso",
        "Dibbins",
        "Anglebert Dinkherhump",
        "Michelle Davis"
    ]));

    // (i) a minimal valid HAR. The evaluator ignores its contents but the loader
    // must parse it with >= 1 entry, else the task errors to score 0.0.
    let har = one_entry_har();

    write_task_output(&task_dir, &response, &har)?;
    println!(
        "wrote {} and {}",
        task_dir.join("agent_response.json").display(),
        task_dir.join("network.har").display()
    );

    // (iii) drive the real evaluator and read the score back.
    println!(
        "running: webarena-verified eval-tasks --output-dir {} --task-ids {PINNED_TASK_ID}",
        root.display()
    );
    match run_eval_tasks(&root, &[PINNED_TASK_ID], None) {
        Ok(results) => {
            let result = results
                .first()
                .ok_or("eval returned no result for the pinned task")?;
            println!(
                "task {} -> status={:?} score={}",
                result.task_id, result.status, result.score
            );
            for ev in &result.evaluators_results {
                println!(
                    "  evaluator {} -> {} ({})",
                    ev.evaluator_name, ev.status, ev.score
                );
            }
            assert!(
                result.is_pass(),
                "pinned RETRIEVE task {PINNED_TASK_ID} must score 1.0, got {}",
                result.score
            );
            println!("\nOK: real WebArena-Verified score is 1.0 from an offline HAR replay.");
        }
        Err(EvalError::BinaryNotFound) => {
            println!(
                "\nSKIP: `webarena-verified` is not on PATH. Install it to run the real eval:\n  \
                 python -m venv venv && . venv/bin/activate && pip install webarena-verified\n  \
                 then re-run: cargo run -p anchortree-cdp --example eval_task"
            );
        }
        Err(other) => return Err(other.into()),
    }

    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// A minimal, valid HAR 1.2 log carrying exactly one entry. Hand-built from the
/// public field surface (no browser): one synthetic GET that loaded 200 OK. This
/// satisfies the runner's "the HAR must parse with at least one entry"
/// precondition for a RETRIEVE replay.
fn one_entry_har() -> Har {
    let url = "http://shopping.local/catalog/product/view/id/21";
    let entry = HarEntry {
        started_date_time: "1970-01-01T00:00:00.000Z".to_string(),
        time: 1.0,
        request: HarRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            http_version: "HTTP/1.1".to_string(),
            cookies: Vec::<HarCookie>::new(),
            headers: vec![HarHeader {
                name: "Host".to_string(),
                value: "shopping.local".to_string(),
            }],
            query_string: Vec::<HarQuery>::new(),
            headers_size: -1,
            body_size: 0,
            post_data: None,
        },
        response: HarResponse {
            status: 200,
            status_text: "OK".to_string(),
            http_version: "HTTP/1.1".to_string(),
            cookies: Vec::<HarCookie>::new(),
            headers: vec![HarHeader {
                name: "Content-Type".to_string(),
                value: "text/html".to_string(),
            }],
            content: HarContent {
                size: 0,
                mime_type: "text/html".to_string(),
                text: None,
                encoding: None,
            },
            redirect_url: String::new(),
            headers_size: -1,
            body_size: 0,
            error: None,
        },
        cache: HarCache {},
        timings: HarTimings {
            blocked: -1.0,
            dns: -1.0,
            connect: -1.0,
            send: 0.0,
            wait: 1.0,
            receive: 0.0,
            ssl: -1.0,
        },
        server_ip_address: None,
    };

    Har {
        log: HarLog {
            version: "1.2",
            creator: HarCreator {
                name: "anchortree".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            entries: vec![entry],
        },
    }
}
