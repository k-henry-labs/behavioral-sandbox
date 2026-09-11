//! `boxdesk serve`: the verb that turns this installed copy into a box.
//!
//! - **It refuses before it listens.** No hypervisor, no token, a token anybody can read: each is
//!   a sentence naming the thing to fix, and nothing binds.
//! - **Loopback unless told otherwise**, and a wider bind is said out loud at startup.
//! - **Everything past this lives in `boxdesk-serve`.** This module is the flags and the refusals.

use std::process::ExitCode;

use boxdesk_serve::config::{self, Config};
use clap::Args;

use crate::EXIT_OPERATIONAL;

#[derive(Args)]
pub(crate) struct ServeArgs {
    /// Where to listen: `PORT`, `127.0.0.1:PORT` or `0.0.0.0:PORT`. A bare port is loopback.
    /// Falls back to `$BOXDESK_SERVE_BIND`, then loopback on 8420.
    #[arg(long, value_name = "ADDR")]
    bind: Option<String>,
    /// Where this server keeps its ledger. Falls back to `$BOXDESK_SERVE_DATA`, then the runs
    /// directory's parent.
    #[arg(long, value_name = "DIR")]
    data: Option<std::path::PathBuf>,
    /// The most sandboxes to run at once.
    #[arg(long, value_name = "N")]
    concurrency: Option<usize>,
}

pub(crate) fn run(args: &ServeArgs) -> ExitCode {
    match serve(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("boxdesk serve: {why}");
            ExitCode::from(EXIT_OPERATIONAL)
        }
    }
}

fn serve(args: &ServeArgs) -> Result<(), config::Refusal> {
    // Asked before anything binds: a caller who posts a job to a box with no hypervisor should be
    // told by the box refusing to start, not by a VM that never boots.
    if let Some(why) = config::hypervisor_unusable() {
        return Err(config::Refusal::NoHypervisor(why));
    }
    let data = args
        .data
        .clone()
        .or_else(|| std::env::var_os(config::DATA_ENV).map(Into::into))
        .unwrap_or_else(default_data);
    let bind = config::bind(args.bind.as_deref(), std::env::var(config::BIND_ENV).ok())?;
    let token = config::token(
        std::env::var(config::TOKEN_ENV).ok(),
        &data.join("serve.token"),
    )?;
    let config = Config {
        bind,
        token,
        data,
        concurrency: args.concurrency.unwrap_or(config::DEFAULT_CONCURRENCY),
    };

    let state = boxdesk_serve::http::Serve::new(config.clone())?;
    eprintln!("boxdesk serve: listening on http://{}", config.bind);
    eprintln!(
        "boxdesk serve: usage counts at {}",
        state.ledger_path().display()
    );
    if config::is_public(&config.bind) {
        eprintln!(
            "boxdesk serve: this reaches past the machine. Anyone who can reach {} and holds the \
             token can run code here.",
            config.bind
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| config::Refusal::Local(format!("start the runtime: {e}")))?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(config.bind)
            .await
            .map_err(|e| config::Refusal::Local(format!("bind {}: {e}", config.bind)))?;
        axum::serve(listener, boxdesk_serve::http::router(state))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| config::Refusal::Local(format!("serve: {e}")))
    })
}

/// Beside the runs, because that is where this machine already keeps what a sandbox left.
fn default_data() -> std::path::PathBuf {
    boxdesk_record::runs_dir()
        .ok()
        .and_then(|runs| runs.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
