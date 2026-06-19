//! Phase 2.2b proof: the opt-in **visual** Set-of-Mark escalation.
//!
//! Every other handle anchortree hands an agent is *textual* — a durable diff
//! plus a short list of `m{n}` mark lines, which is an order of magnitude cheaper
//! in tokens than a screenshot (`DECISIONS.md` D13). This example exercises the
//! rare escalation for the genuinely DOM-less case: it captures a page
//! screenshot and overlays a numbered box on each mark, aligned to the exact
//! geometry the text path uses.
//!
//! It runs over the hosted connect leg ([`connect_hosted`]) so it drives a
//! locally launched Chrome and a hosted gateway through the same path. It
//! observes a page, takes the marks the engine minted, draws the overlay with
//! [`screenshot_with_marks`], and writes the composite PNG next to the textual
//! render so the two can be compared side by side.
//!
//! ## Running it
//!
//! Build it with the feature on (it is excluded from a default build by
//! `required-features`):
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --features visual-marks --example visual_marks
//! ```
//!
//! With no endpoint configured it prints usage and exits 0, so it stays
//! unattended-safe and still type-checks the whole visual path in CI.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{MarkOverlay, connect_hosted};
use anchortree_core::{IdentityMap, ObservationSource as _};

/// A page deliberately seeded with a couple of unlabeled icon buttons — the kind
/// of element that becomes a transient mark rather than a durable eid, because
/// there is no stable accessible name to anchor an identity to.
const JS_PAGE: &str = r#"document.body.innerHTML = `
  <main style="font-family:sans-serif;padding:24px">
    <h1>Toolbar</h1>
    <button id="save" aria-label="Save">Save</button>
    <span role="button" tabindex="0" style="display:inline-block;width:40px;height:40px;background:#c0392b;margin:8px"></span>
    <span role="button" tabindex="0" style="display:inline-block;width:40px;height:40px;background:#2980b9;margin:8px"></span>
  </main>`; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some(ws_url) = resolve_ws_url()? else {
        print_usage();
        return Ok(());
    };

    println!("connecting (hosted leg) to {ws_url}");
    let mut session = connect_hosted(&ws_url).await?;
    session.navigate("about:blank").await?;
    session.evaluate(JS_PAGE).await?;

    let mut map = IdentityMap::new();
    let obs = map.observe(session.observer.observe().await?);

    println!("\ntextual handle surface (the canonical, token-cheap path):");
    print!("{}", obs.render());

    if obs.marks.is_empty() {
        println!(
            "\nthis page minted no transient marks, so there is nothing to overlay. \
             The visual escalation only applies when the engine produces marks."
        );
        return Ok(());
    }

    let png = session
        .observer
        .screenshot_with_marks(&obs.marks, MarkOverlay::default())
        .await?;
    let out = "visual_marks.png";
    std::fs::write(out, &png)?;
    println!(
        "\nwrote {} ({} bytes): a screenshot with {} numbered box(es), one per mark above.",
        out,
        png.len(),
        obs.marks.len()
    );
    println!("OK: the visual set-of-mark overlay aligns to the same marks the text path mints.");
    Ok(())
}

/// Resolve a `ws://`/`wss://` CDP URL from `ANCHORTREE_CDP_WS`, or derive one
/// from `ANCHORTREE_CDP_HTTP` via `/json/version`. `Ok(None)` means nothing was
/// configured.
fn resolve_ws_url() -> Result<Option<String>, Box<dyn Error>> {
    if let Ok(ws) = std::env::var("ANCHORTREE_CDP_WS") {
        if !ws.is_empty() {
            return Ok(Some(ws));
        }
    }
    if let Ok(http) = std::env::var("ANCHORTREE_CDP_HTTP") {
        if !http.is_empty() {
            return Ok(Some(fetch_ws_debugger_url(&http)?));
        }
    }
    Ok(None)
}

fn print_usage() {
    eprintln!(
        "visual_marks: configure an endpoint to run the visual set-of-mark proof.\n\
         \n\
         Local headless Chrome (ws://):\n  \
         ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \\\n    \
         cargo run -p anchortree-cdp --features visual-marks --example visual_marks\n\
         \n\
         No endpoint configured, nothing to do. Exiting 0."
    );
}

/// Issue a minimal blocking `GET /json/version` and pull out
/// `webSocketDebuggerUrl`. Dependency-free, matching the other examples.
fn fetch_ws_debugger_url(http_endpoint: &str) -> Result<String, Box<dyn Error>> {
    let host_port = http_endpoint
        .strip_prefix("http://")
        .ok_or("ANCHORTREE_CDP_HTTP must start with http://")?
        .trim_end_matches('/');

    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "GET /json/version HTTP/1.1\r\nHost: {host_port}\r\nAccept: application/json\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err("CDP endpoint closed before sending full headers".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("CDP /json/version response had no Content-Length")?;

    while buf.len() < header_end + content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[header_end..(header_end + content_length).min(buf.len())];
    let json: serde_json::Value = serde_json::from_slice(body)?;
    let ws = json
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or("CDP /json/version response had no webSocketDebuggerUrl")?;
    Ok(ws.to_string())
}

/// Index of the first occurrence of `needle` in `haystack`, if any.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
