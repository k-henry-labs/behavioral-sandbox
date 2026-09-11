//! The listener: a posture and a command in, a record out.
//!
//! - **A job is run by the binary this is.** `serve` re-executes its own `tormoni` with the flags
//!   a person would have typed, rather than driving the supervisor itself. That is the whole
//!   reason a served record is byte for byte a local one: there is one run path, not two that
//!   agree today.
//! - **The stream is newline-delimited JSON.** One object per event, ending with the run. A caller
//!   waiting on a sixty-second build sees output as it happens, which is the difference between
//!   usable and not.
//! - **The posture is passed through, never relaxed.** Every field of a request becomes the flag
//!   of the same name. There is no server-side default that differs from the CLI's.
//! - **Refusals are RFC 9457.** `detail` is written to be read by a person, because the client on
//!   the other end prints it.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;

use crate::config::{self, Config};
use crate::meter::{Allocation, Ledger, Meter, TICK};

/// Everything a request needs, shared by every handler.
pub struct Serve {
    config: Config,
    ledger: Ledger,
    /// The bound on how many sandboxes run at once, held as a permit for the life of each.
    running: Arc<Semaphore>,
    /// The `tormoni` this process is, which is what actually boots anything.
    tormoni: PathBuf,
}

impl Serve {
    /// The state for `config`, with the ledger beside the runs it describes.
    pub fn new(config: Config) -> Result<Arc<Self>, config::Refusal> {
        let ledger = Ledger::at(config.data.join("usage.jsonl"))
            .map_err(|e| config::Refusal::Local(format!("open the ledger: {e}")))?;
        let tormoni = std::env::current_exe()
            .map_err(|e| config::Refusal::Local(format!("find this binary: {e}")))?;
        Ok(Arc::new(Self {
            running: Arc::new(Semaphore::new(config.concurrency)),
            config,
            ledger,
            tormoni,
        }))
    }

    /// Where the counts go, for the line an operator sees at startup.
    #[must_use]
    pub fn ledger_path(&self) -> &std::path::Path {
        self.ledger.path()
    }
}

/// Every lane this box serves.
pub fn router(state: Arc<Serve>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/runs", post(run_job))
        .route("/v1/runs/{id}/archive", get(archive))
        .route("/v1/sandboxes", post(start_sandbox))
        .route("/v1/sandboxes/{name}/exec", post(exec_in))
        .route("/v1/sandboxes/{name}", delete(stop_sandbox))
        .with_state(state)
}

/// Whether this box is up. The one lane with no bearer: a load balancer has none.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// The posture and command a caller asks for: every field is the flag of the same name.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// The command, one word per element. The guest resolves the first word.
    pub command: Vec<String>,
    pub name: Option<String>,
    pub root: Option<String>,
    pub vcpus: Option<u32>,
    pub mem_mib: Option<u32>,
    pub workdir: Option<String>,
    /// `guest=host`, as `--mount` takes it.
    #[serde(default)]
    pub mounts: Vec<String>,
    /// `tag=host`, as `--share` takes it.
    #[serde(default)]
    pub shares: Vec<String>,
    /// `none` or `tsi`. Absent is the CLI's own default, which is `none`.
    pub net: Option<String>,
    /// `read-only` or `writable`.
    pub rootfs: Option<String>,
    /// `KEY=VALUE`, as `--env` takes it. The guest gets the whole entry; the record keeps the name.
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub no_results: bool,
}

impl Job {
    /// The argv a person would have typed. One place, so a posture cannot mean one thing here and
    /// another at a keyboard.
    fn argv(&self, verb: &str, name: &str) -> Vec<String> {
        let mut argv = vec![verb.to_string(), "--name".to_string(), name.to_string()];
        // A run is ephemeral at a keyboard, and a served one cannot be: the archive lane hands
        // back the bytes `tormoni export` writes, and there is nothing to export from a directory
        // the run took with it. The box sweeps its own after a caller has had them.
        if verb == "run" {
            argv.push("--keep".to_string());
        }
        let mut push = |flag: &str, value: &str| {
            argv.push(flag.to_string());
            argv.push(value.to_string());
        };
        if let Some(root) = &self.root {
            push("--root", root);
        }
        if let Some(vcpus) = self.vcpus {
            push("--vcpus", &vcpus.to_string());
        }
        if let Some(mem) = self.mem_mib {
            push("--mem", &mem.to_string());
        }
        if let Some(workdir) = &self.workdir {
            push("--workdir", workdir);
        }
        for mount in &self.mounts {
            push("--mount", mount);
        }
        for share in &self.shares {
            push("--share", share);
        }
        if let Some(net) = &self.net {
            push("--net", net);
        }
        if let Some(rootfs) = &self.rootfs {
            push("--rootfs", rootfs);
        }
        for entry in &self.env {
            push("--env", entry);
        }
        if self.no_results {
            argv.push("--no-results".to_string());
        }
        if !self.command.is_empty() {
            argv.push("--".to_string());
            argv.extend(self.command.iter().cloned());
        }
        argv
    }

    /// What this job holds while it runs. The CLI's own defaults, spelled here because a meter
    /// needs a number before the record exists.
    fn allocation(&self) -> Allocation {
        Allocation {
            vcpus: self.vcpus.unwrap_or(1),
            mem_mib: self.mem_mib.unwrap_or(512),
            // The default posture holds no writable disk: the root is read-only and `/results`
            // lives in the record directory, which is the host's.
            disk_mib: 0,
        }
    }
}

/// `POST /v1/runs` — one ephemeral sandbox, streamed while it runs.
///
/// The response is newline-delimited JSON: a `started`, then `stdout` and `stderr` as they arrive,
/// then an `ended` carrying how it finished and what it consumed.
async fn run_job(
    State(state): State<Arc<Serve>>,
    headers: HeaderMap,
    Json(job): Json<Job>,
) -> Response {
    if let Some(refusal) = authorize(&state, &headers) {
        return refusal;
    }
    if job.command.is_empty() {
        return problem(
            StatusCode::BAD_REQUEST,
            "no command",
            "A job has to say what to run: `command` is a list of words, and the guest resolves \
             the first one.",
        );
    }
    let Ok(permit) = state.running.clone().try_acquire_owned() else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "at capacity",
            format!(
                "This box is already running the {} sandboxes it is configured for. Nothing was \
                 started; try again when one ends.",
                state.config.concurrency
            ),
        );
    };

    let name = job
        .name
        .clone()
        .unwrap_or_else(|| format!("job-{}", tormoni_record::now_ms()));
    let argv = job.argv("run", &name);
    let mut child = match tokio::process::Command::new(&state.tormoni)
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not start",
                format!("This box could not start a sandbox: {e}"),
            );
        }
    };

    let (sender, events) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(64);
    let out = child.stdout.take();
    let err = child.stderr.take();
    let allocation = job.allocation();
    let ledger = state.ledger.clone();
    let tormoni = state.tormoni.clone();

    tokio::spawn(async move {
        // The permit is held for the whole of the run and dropped with this task, so the bound
        // counts sandboxes rather than requests.
        let _permit = permit;
        let mut meter = Meter::started(&name, allocation, ledger);
        let _ = sender
            .send(Ok(line(&json!({ "event": "started", "name": name }))))
            .await;

        // One pump per stream, generic over which kind of pipe it is: a closure would fix the
        // reader's type to whichever was passed first.
        fn pump<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
            reader: Option<R>,
            which: &'static str,
            to: tokio::sync::mpsc::Sender<Result<String, std::io::Error>>,
        ) -> tokio::task::JoinHandle<()> {
            tokio::spawn(async move {
                let Some(reader) = reader else { return };
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(text)) = lines.next_line().await {
                    if to
                        .send(Ok(line(&json!({ "event": which, "text": text }))))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            })
        }
        let pumping_out = pump(out, "stdout", sender.clone());
        let pumping_err = pump(err, "stderr", sender.clone());

        // Ticked while it runs, so a box that loses power has already written down the time this
        // sandbox held it.
        let ended = loop {
            tokio::select! {
                status = child.wait() => break status,
                () = tokio::time::sleep(TICK) => { let _ = meter.tick(); }
            }
        };
        let _ = pumping_out.await;
        let _ = pumping_err.await;
        let units = meter.tick().unwrap_or_else(|_| crate::meter::Units::none());

        let record = show(&tormoni, &name).await;
        let _ = sender
            .send(Ok(line(&json!({
                "event": "ended",
                "exit_status": ended.ok().and_then(|s| s.code()),
                "run": record,
                "units": {
                    "seconds": units.seconds,
                    "vcpu_seconds": units.vcpu_seconds,
                    "memory_gib_seconds": units.memory_gib_seconds,
                    "disk_gib_seconds": units.disk_gib_seconds,
                },
            }))))
            .await;
    });

    let stream = Events(events);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-store")
        .body(axum::body::Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `GET /v1/runs/{id}/archive` — the ustar `tormoni export` writes, unchanged.
///
/// Written by the binary itself into a scratch file and handed back whole: this must be the same
/// bytes a person would have got at a keyboard, so nothing here assembles an archive of its own.
async fn archive(
    State(state): State<Arc<Serve>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Some(refusal) = authorize(&state, &headers) {
        return refusal;
    }
    let scratch = state.config.data.join("export");
    if let Err(e) = tokio::fs::create_dir_all(&scratch).await {
        return problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not write",
            format!("This box could not make room to export a run: {e}"),
        );
    }
    let out = tokio::process::Command::new(&state.tormoni)
        .arg("export")
        .arg(&id)
        .arg("--to")
        .arg(&scratch)
        .output()
        .await;
    let Ok(out) = out else {
        return problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not export",
            "This box could not run its own export.",
        );
    };
    if !out.status.success() {
        return problem(
            StatusCode::NOT_FOUND,
            "no such run",
            String::from_utf8_lossy(&out.stderr).trim(),
        );
    }
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not read",
            "This box exported a run and could not read it back.",
        );
    };
    let _ = tokio::fs::remove_file(&path).await;
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-tar".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{id}.tar\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `POST /v1/sandboxes` — one that outlives its command.
async fn start_sandbox(
    State(state): State<Arc<Serve>>,
    headers: HeaderMap,
    Json(job): Json<Job>,
) -> Response {
    if let Some(refusal) = authorize(&state, &headers) {
        return refusal;
    }
    let Some(name) = job.name.clone() else {
        return problem(
            StatusCode::BAD_REQUEST,
            "no name",
            "A sandbox that outlives its command is reached by name afterwards, so it needs one: \
             send `name`.",
        );
    };
    // `up` takes no command; a persistent sandbox is started and then execed into.
    let mut argv = job.argv("up", &name);
    if let Some(at) = argv.iter().position(|a| a == "--") {
        argv.truncate(at);
    }
    match tokio::process::Command::new(&state.tormoni)
        .args(&argv)
        .output()
        .await
    {
        Ok(out) if out.status.success() => (
            StatusCode::CREATED,
            Json(json!({ "name": name, "started": true })),
        )
            .into_response(),
        Ok(out) => problem(
            StatusCode::BAD_REQUEST,
            "could not start",
            String::from_utf8_lossy(&out.stderr).trim(),
        ),
        Err(e) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not start",
            format!("This box could not start a sandbox: {e}"),
        ),
    }
}

/// `POST /v1/sandboxes/{name}/exec` — another command in one already up.
async fn exec_in(
    State(state): State<Arc<Serve>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(job): Json<Job>,
) -> Response {
    if let Some(refusal) = authorize(&state, &headers) {
        return refusal;
    }
    if job.command.is_empty() {
        return problem(
            StatusCode::BAD_REQUEST,
            "no command",
            "An exec has to say what to run: `command` is a list of words.",
        );
    }
    let mut argv = vec!["exec".to_string(), name, "--".to_string()];
    argv.extend(job.command.iter().cloned());
    match tokio::process::Command::new(&state.tormoni)
        .args(&argv)
        .output()
        .await
    {
        Ok(out) => (
            StatusCode::OK,
            Json(json!({
                "exit_status": out.status.code(),
                "stdout": String::from_utf8_lossy(&out.stdout),
                "stderr": String::from_utf8_lossy(&out.stderr),
            })),
        )
            .into_response(),
        Err(e) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not exec",
            format!("This box could not reach that sandbox: {e}"),
        ),
    }
}

/// `DELETE /v1/sandboxes/{name}` — stop one that is up.
async fn stop_sandbox(
    State(state): State<Arc<Serve>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if let Some(refusal) = authorize(&state, &headers) {
        return refusal;
    }
    match tokio::process::Command::new(&state.tormoni)
        .args(["stop", &name])
        .output()
        .await
    {
        Ok(out) if out.status.success() => StatusCode::NO_CONTENT.into_response(),
        Ok(out) => problem(
            StatusCode::NOT_FOUND,
            "no such sandbox",
            String::from_utf8_lossy(&out.stderr).trim(),
        ),
        Err(e) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not stop",
            format!("This box could not stop that sandbox: {e}"),
        ),
    }
}

/// The run as the binary's own `--json` describes it, or null where it could not be read.
async fn show(tormoni: &std::path::Path, name: &str) -> Value {
    let out = tokio::process::Command::new(tormoni)
        .args(["show", "--json", name])
        .output()
        .await;
    out.ok()
        .filter(|out| out.status.success())
        .and_then(|out| serde_json::from_slice(&out.stdout).ok())
        .unwrap_or(Value::Null)
}

/// One event, as a line of the stream.
fn line(value: &Value) -> String {
    format!("{value}\n")
}

/// The events channel, as the stream axum sends.
///
/// Hand-written rather than taken from a combinator crate: one `poll_recv` is the whole of what
/// this needs, and a dependency for it would be a dependency for nothing.
struct Events(tokio::sync::mpsc::Receiver<Result<String, std::io::Error>>);

impl futures_core::Stream for Events {
    type Item = Result<String, std::io::Error>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// The refusal for a request without this box's bearer, or nothing where it carried one.
///
/// An `Option` rather than a `Result`: this is a guard, and the refusal IS the response, so a
/// `Result` would be carrying a whole `Response` in an error variant for no reason.
fn authorize(state: &Serve, headers: &HeaderMap) -> Option<Response> {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let (scheme, token) = v.split_at_checked(7)?;
            scheme.eq_ignore_ascii_case("bearer ").then_some(token)
        })
        .map(str::trim)
        .unwrap_or_default();
    if config::accepts(&state.config.token, presented) {
        return None;
    }
    Some(problem(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "This box takes a bearer token. The one it accepts is the one in its token file.",
    ))
}

/// RFC 9457, because a client on the other end prints `detail` as it arrived.
fn problem(status: StatusCode, title: &str, detail: impl AsRef<str>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        Json(json!({
            "type": format!("/errors/{}", title.replace(' ', "-")),
            "title": title,
            "status": status.as_u16(),
            "detail": detail.as_ref(),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A posture becomes the flags a person would have typed**, so a served run cannot be
    /// quietly less isolated than a local one. Every field is passed through; none is defaulted
    /// here, because a default written twice is a default that drifts.
    #[test]
    fn a_job_becomes_the_argv_a_person_would_have_typed() {
        let job = Job {
            command: vec!["sh".into(), "-c".into(), "echo hi".into()],
            vcpus: Some(2),
            mem_mib: Some(1024),
            mounts: vec!["/mnt=/srv/code".into()],
            net: Some("tsi".into()),
            rootfs: Some("writable".into()),
            env: vec!["CI=1".into()],
            ..Job::default()
        };
        assert_eq!(
            job.argv("run", "job-1"),
            [
                "run",
                "--name",
                "job-1",
                "--keep",
                "--vcpus",
                "2",
                "--mem",
                "1024",
                "--mount",
                "/mnt=/srv/code",
                "--net",
                "tsi",
                "--rootfs",
                "writable",
                "--env",
                "CI=1",
                "--",
                "sh",
                "-c",
                "echo hi"
            ]
        );
    }

    /// A job that says nothing about the network gets no `--net`, so the CLI's own default
    /// decides. The server never writes a posture default of its own.
    #[test]
    fn an_unstated_posture_is_left_to_the_cli_to_default() {
        let job = Job {
            command: vec!["true".into()],
            ..Job::default()
        };
        let argv = job.argv("run", "job-1");
        for flag in ["--net", "--rootfs", "--vcpus", "--mem", "--root"] {
            assert!(
                !argv.iter().any(|a| a == flag),
                "{flag} was invented: {argv:?}"
            );
        }
        assert_eq!(argv, ["run", "--name", "job-1", "--keep", "--", "true"]);
    }

    /// An allocation is what a job asked for, and the CLI's defaults where it asked for nothing:
    /// a meter needs a number before a record exists to read one from.
    #[test]
    fn an_allocation_is_what_was_asked_for_or_the_default() {
        let asked = Job {
            vcpus: Some(4),
            mem_mib: Some(8192),
            ..Job::default()
        };
        assert_eq!(
            asked.allocation(),
            Allocation {
                vcpus: 4,
                mem_mib: 8192,
                disk_mib: 0
            }
        );
        assert_eq!(
            Job::default().allocation(),
            Allocation {
                vcpus: 1,
                mem_mib: 512,
                disk_mib: 0
            }
        );
    }
}
