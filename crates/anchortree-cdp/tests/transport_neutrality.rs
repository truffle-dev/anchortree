//! Phase 3.4: the transport-neutrality guard (D9, D31).
//!
//! anchortree's cross-browser claim rests on one structural invariant: CDP types
//! live only in the thin adapter files and are decoded into the plain `RawAxNode`
//! / `RawAttrs` value structs at `observer.rs` before fusion. Nothing in the
//! fusion path, and nothing in `anchortree-core`, may name `chromiumoxide`. Until
//! now that boundary was verified by hand every build (a grep in the run notes).
//! This test makes it a fitness function: it scans both crates' real source and
//! fails the build if a CDP type ever crosses the seam.
//!
//! Two halves, matching the two halves of D9:
//!
//! 1. **`anchortree-core` is CDP-free, full stop.** The engine crate does not
//!    even depend on `chromiumoxide`; this asserts no source line reintroduces
//!    it, so a future "just import the CDP type here" shortcut fails loudly.
//! 2. **Inside `anchortree-cdp`, the CDP surface is exactly the transport
//!    adapters** — and the fusion / metric / report path (`fuse.rs`, `eval.rs`,
//!    `report.rs`) is clean. A new file that names a CDP type, or a leak into the
//!    fusion path, breaks the pinned partition and forces a deliberate decision.
//!
//! Why a source scan rather than a type-level trick: the leak we guard against is
//! a *human* one — someone reaching for `chromiumoxide::...::BackendNodeId` in
//! `fuse.rs` because it is one import away. A scan catches that at the only place
//! it can be caught, the text, and reads as an architecture rule in the test
//! output. The rule deliberately ignores comment lines so the doc prose in
//! `lib.rs` / `frames.rs` ("decoded from chromiumoxide in observer.rs") is not a
//! false positive; only code references count.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The transport-adapter files in `anchortree-cdp/src` that are *allowed* to name
/// CDP types in code. This is the seam: every other file in the crate, and all of
/// `anchortree-core`, must be CDP-free. Keep this list honest — adding to it is a
/// deliberate choice to widen the transport surface, not a rubber stamp.
const CDP_ADAPTER_FILES: &[&str] = &[
    "actions.rs",  // trusted-gesture dispatch over the CDP Input domain
    "channel.rs",  // the CDP transport abstraction (local Page / hosted session)
    "error.rs",    // wraps chromiumoxide's CdpError as the crate error
    "fulfill.rs",  // maps a matcher verdict to Fetch.fulfillRequest/failRequest params
    "har.rs",      // decodes CDP Network.* events into a transport-neutral HAR
    "observer.rs", // the decode boundary: getFullAXTree/DOM -> RawAxNode (D9)
    "runner.rs",   // drives the local chromiumoxide::Page event loop for capture
];

/// The fusion / metric / report path that the engine's transport-neutrality
/// depends on most directly. Called out separately so a leak here fails with a
/// pointed message rather than just "the partition changed".
const FUSION_PATH_FILES: &[&str] = &["fuse.rs", "eval.rs", "report.rs", "corpus.rs", "replay.rs"];

/// Whether `src` references `chromiumoxide` in *code* (not in a comment). A line
/// counts only when, with leading whitespace trimmed, it does not begin with
/// `//` — which excludes line comments and all three doc-comment forms
/// (`//!`, `///`, `//`). Block comments are not used around any CDP mention in
/// this crate, so this single rule is sufficient and intentionally strict.
fn code_names_chromiumoxide(src: &str) -> bool {
    src.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
        .any(|line| line.contains("chromiumoxide"))
}

/// Every `.rs` file under `dir`, recursively, as `(file_name, contents)`.
fn rust_sources(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path).expect("source dir is readable") {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let body = fs::read_to_string(&p).expect("source file is utf-8");
                out.push((name, body));
            }
        }
    }
    out
}

/// `crates/anchortree-cdp` at build time; its parent is the workspace `crates/`.
fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn anchortree_core_names_no_cdp_type() {
    let core_src = crate_dir().join("../anchortree-core/src");
    let offenders: BTreeSet<String> = rust_sources(&core_src)
        .into_iter()
        .filter(|(_, body)| code_names_chromiumoxide(body))
        .map(|(name, _)| name)
        .collect();
    assert!(
        offenders.is_empty(),
        "anchortree-core must never name a CDP type (D9); found in: {offenders:?}"
    );
}

#[test]
fn cdp_surface_is_exactly_the_transport_adapters() {
    let cdp_src = crate_dir().join("src");
    let actual: BTreeSet<String> = rust_sources(&cdp_src)
        .into_iter()
        .filter(|(_, body)| code_names_chromiumoxide(body))
        .map(|(name, _)| name)
        .collect();
    let expected: BTreeSet<String> = CDP_ADAPTER_FILES.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        actual, expected,
        "the CDP code surface drifted from the pinned transport adapters (D9/D31).\n\
         A file added here means a new transport-touching module: either it belongs \
         behind the seam (decode to RawAxNode in observer.rs) or CDP_ADAPTER_FILES \
         must be widened deliberately."
    );
}

#[test]
fn fusion_path_is_cdp_free() {
    let cdp_src = crate_dir().join("src");
    let sources = rust_sources(&cdp_src);
    for file in FUSION_PATH_FILES {
        let (_, body) = sources
            .iter()
            .find(|(name, _)| name == file)
            .unwrap_or_else(|| panic!("{file} exists in the cdp crate"));
        assert!(
            !code_names_chromiumoxide(body),
            "{file} is on the fusion/metric/report path and must stay CDP-free (D9): \
             a transport type leaked past the observer.rs decode boundary"
        );
    }
}
