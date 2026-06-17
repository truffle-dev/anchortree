//! Phase 3.1 proof: turn *provider credentials* into a self-authenticating
//! `wss://` CDP URL via [`anchortree_cdp::gateway`].
//!
//! A hosted gateway does not hand you a bare WebSocket URL. You authenticate to
//! its HTTP control plane first, and it returns a URL whose credential rides in
//! the URL itself (never a header — see `DECISIONS.md` D18). This example proves
//! that acquire leg end to end against a real provider:
//!
//! - **Browserbase** when `BROWSERBASE_API_KEY` and `BROWSERBASE_PROJECT_ID`
//!   are set: mints a session over REST (`gateway::browserbase::acquire`) and
//!   prints the returned `connectUrl` plus a session-replay link.
//! - **Cloudflare Browser Run** when `CLOUDFLARE_ACCOUNT_ID` and
//!   `CLOUDFLARE_BROWSER_TOKEN` are set (a token with Browser Rendering - Edit):
//!   builds the `?token=` URL with no round-trip
//!   (`gateway::cloudflare::devtools_ws_url`).
//!
//! With neither pair set it prints usage and exits 0, so it is unattended-safe
//! and still compiles in CI (where the acquire wiring is type-checked).
//!
//! ```text
//! BROWSERBASE_API_KEY=… BROWSERBASE_PROJECT_ID=… \
//!     cargo run -p anchortree-cdp --example observe_hosted
//! ```
//!
//! ## Scope: the acquire leg, not the connect leg
//!
//! This example stops once it holds the self-authenticating URL, because wiring
//! the *connect* leg through chromiumoxide against a hosted browser is a known
//! open problem tracked separately (`STATE.md` / `DECISIONS.md` D19). The TLS
//! connect path itself is exercised by `observe_wss`; the obstacle is specific
//! to reusing the page a hosted browser already has open, which chromiumoxide
//! 0.9.1 does not support cleanly. The credential-to-URL step proven here is the
//! piece a caller cannot write themselves without re-deriving each provider's
//! control-plane API.

use std::error::Error;

use anchortree_cdp::gateway;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some((provider, acquired)) = resolve_provider().await? else {
        print_usage();
        return Ok(());
    };

    println!("provider: {provider}");
    println!("acquired a self-authenticating CDP URL from provider credentials");
    println!(
        "  connect URL: {}",
        redact_credential(&acquired.connect_url)
    );
    if let Some(id) = &acquired.session_id {
        println!("  session id:  {id}");
        println!("  replay:      https://browserbase.com/sessions/{id}");
    }

    assert!(
        acquired.connect_url.starts_with("wss://"),
        "a hosted gateway must yield a TLS WebSocket URL, got: {}",
        acquired.connect_url
    );

    println!(
        "\nOK: provider credentials resolved to a wss:// CDP URL ({provider}).\n\
         The connect+rebind leg over this URL is tracked as the next increment."
    );
    Ok(())
}

/// Resolve a provider from the environment to its acquired session. `Ok(None)`
/// means no provider was configured.
async fn resolve_provider()
-> Result<Option<(&'static str, gateway::AcquiredSession)>, Box<dyn Error>> {
    if let (Some(key), Some(project)) = (env("BROWSERBASE_API_KEY"), env("BROWSERBASE_PROJECT_ID"))
    {
        println!("minting a Browserbase session over REST…");
        let acquired = gateway::browserbase::acquire(&project, &key).await?;
        return Ok(Some(("Browserbase", acquired)));
    }

    if let (Some(account), Some(token)) = (
        env("CLOUDFLARE_ACCOUNT_ID"),
        env("CLOUDFLARE_BROWSER_TOKEN"),
    ) {
        // No round-trip: the URL is itself the credential.
        let url = gateway::cloudflare::devtools_ws_url(&account, &token);
        let acquired = gateway::AcquiredSession {
            connect_url: url,
            session_id: None,
        };
        return Ok(Some(("Cloudflare Browser Run", acquired)));
    }

    Ok(None)
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Mask the credential that rides in a hosted-gateway URL (the `apiKey` /
/// `token` query value) so a printed URL never leaks a live secret.
fn redact_credential(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    let (head, query) = match url.split_once('?') {
        Some(parts) => parts,
        None => return url.to_string(),
    };
    out.push_str(head);
    out.push('?');
    for (i, pair) in query.split('&').enumerate() {
        if i > 0 {
            out.push('&');
        }
        match pair.split_once('=') {
            Some((k, _)) => {
                out.push_str(k);
                out.push_str("=<redacted>");
            }
            None => out.push_str(pair),
        }
    }
    out
}

fn print_usage() {
    eprintln!(
        "observe_hosted: set one provider's credentials to run the acquire proof.\n\
         \n\
         Browserbase:\n  \
         BROWSERBASE_API_KEY=<key> BROWSERBASE_PROJECT_ID=<id> \\\n    \
         cargo run -p anchortree-cdp --example observe_hosted\n\
         \n\
         Cloudflare Browser Run (token: Browser Rendering - Edit):\n  \
         CLOUDFLARE_ACCOUNT_ID=<acct> CLOUDFLARE_BROWSER_TOKEN=<token> \\\n    \
         cargo run -p anchortree-cdp --example observe_hosted\n\
         \n\
         No provider configured, nothing to do. Exiting 0."
    );
}
