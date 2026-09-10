//! `tormoni cloud`: the far end of a run, for a person who has one.
//!
//! - **A namespace, because the words are taken.** `ls`, `show` and `rm` already name what this
//!   machine holds. Everything under `cloud` speaks to the console instead, the way `git remote`
//!   divides the same two ideas.
//! - **Two strings and no model.** This holds an opaque token and an origin, and no type for who
//!   anybody is or what they may do; `--account` is a string the server printed, copied back.
//!   Design rule 3 survives because every permission answer is a 403 the console sent.
//! - **The archive is the wire format.** A push is the bytes `tormoni export` writes, unchanged,
//!   and a pull is those bytes back. `a_pulled_archive_is_the_bytes_that_were_pushed` is the
//!   round trip that holds them equal.
//! - **The server's words, not ours.** A refusal is RFC 9457 and its `detail` is written for a
//!   person; it is printed as it arrived. A second sentence here would drift from it.
//! - **`curl`, like the app's console calls.** The bearer travels on curl's stdin, never in argv,
//!   because a process list is public.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::{Args, Subcommand};

use crate::EXIT_OPERATIONAL;

/// The console's origin when nothing else names one.
const DEFAULT_CONSOLE: &str = "https://tormoni.ai";
/// The origin, when the flag is absent.
const CONSOLE_ENV: &str = "TORMONI_CONSOLE";
/// The token, for a machine with no app signed in: CI, mostly.
const TOKEN_ENV: &str = "TORMONI_TOKEN";
/// How long the console gets to answer a read, in seconds.
const PATIENCE: &str = "30";
/// How long it gets to take an archive, which may be tens of MiB.
const PUSH_PATIENCE: &str = "300";

/// What a refusal exits with, so a script can tell them apart without reading the message.
///
/// [`EXIT_OPERATIONAL`] stays what it is everywhere else in this binary: this machine could not
/// do the thing. These are the console's answers, which are a different kind of no.
const EXIT_REFUSED: u8 = 3;
const EXIT_NOT_FOUND: u8 = 4;
const EXIT_TOO_LARGE: u8 = 5;
const EXIT_CONFLICT: u8 = 6;
const EXIT_SERVER: u8 = 7;

#[derive(Args)]
pub(crate) struct CloudArgs {
    /// The console's origin. Else `$TORMONI_CONSOLE`, else https://tormoni.ai.
    #[arg(long, global = true, value_name = "URL")]
    console: Option<String>,
    /// Act on an account you are a member of, by the id its list printed.
    #[arg(long, global = true, value_name = "ID")]
    account: Option<String>,
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Send one run's export to the console. Asks first, so a stored run costs no upload.
    Push(PushArgs),
    /// List the runs the console holds, newest first.
    Ls(LsArgs),
    /// Show one stored run: its posture, how it ended, and what it wrote.
    Show(OneArgs),
    /// Write one stored run's archive back to a file, byte for byte.
    Pull(PullArgs),
    /// Print what one stored run printed.
    Output(OneArgs),
    /// Remove one stored run and its archive.
    Rm(OneArgs),
    /// Add labels to a stored run.
    Label(LabelArgs),
    /// Take one label off a stored run.
    Unlabel(UnlabelArgs),
    /// Every label in use, with how many runs carry it.
    Labels,
    /// Take runs out of every member's sight. The account's owner only.
    Hide(VisibilityArgs),
    /// Put hidden runs back in sight. The account's owner only.
    Unhide(VisibilityArgs),
}

#[derive(Args)]
struct PushArgs {
    /// The run to send, by its id or its name.
    key: String,
    /// A label to land it with. Repeatable.
    #[arg(long, value_name = "LABEL")]
    label: Vec<String>,
    /// Send it even if the console says it already has this run.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct LsArgs {
    /// Continue from where a previous page ended.
    #[arg(long, value_name = "CURSOR")]
    after: Option<String>,
    /// Only the runs carrying this label.
    #[arg(long, value_name = "LABEL")]
    label: Option<String>,
    /// Follow the cursor to the end rather than printing one page.
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct OneArgs {
    /// The run, by the id the console gave it or the id the sandbox did.
    id: String,
}

#[derive(Args)]
struct PullArgs {
    /// The run, by either of its ids.
    id: String,
    /// Where to write the archive. A directory takes `<id>.tar`.
    #[arg(short = 'o', long = "out", value_name = "PATH")]
    out: PathBuf,
}

#[derive(Args)]
struct LabelArgs {
    /// The run, by either of its ids.
    id: String,
    /// The labels to add.
    #[arg(required = true, value_name = "LABEL")]
    labels: Vec<String>,
}

#[derive(Args)]
struct UnlabelArgs {
    /// The run, by either of its ids.
    id: String,
    /// The label to take off.
    label: String,
}

#[derive(Args)]
struct VisibilityArgs {
    /// The runs, by either of their ids.
    #[arg(required = true, value_name = "ID")]
    ids: Vec<String>,
}

pub(crate) fn run(args: &CloudArgs) -> ExitCode {
    let outcome = match console(args.console.as_deref(), std::env::var(CONSOLE_ENV).ok()) {
        Ok(console) => dispatch(args, &console),
        Err(why) => Err(Refusal::local(why)),
    };
    match outcome {
        Ok(said) => {
            if !said.is_empty() {
                println!("{said}");
            }
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            eprintln!("tormoni cloud: {}", refusal.said);
            ExitCode::from(refusal.code)
        }
    }
}

fn dispatch(args: &CloudArgs, console: &str) -> Result<String, Refusal> {
    let at = &Console {
        origin: console.to_string(),
        account: args.account.clone(),
        token: token()?,
    };
    match &args.verb {
        Verb::Push(push) => push_run(at, push),
        Verb::Ls(ls) => list(at, ls),
        Verb::Show(one) => Ok(get(at, &format!("/v1/runs/{}", one.id), &[])?.body),
        Verb::Output(one) => Ok(get(at, &format!("/v1/runs/{}/output", one.id), &[])?.body),
        Verb::Pull(pull) => pull_archive(at, pull),
        Verb::Rm(one) => {
            send(at, "DELETE", &format!("/v1/runs/{}", one.id), None, &[])?;
            Ok(one.id.clone())
        }
        Verb::Label(add) => {
            let body = serde_json::json!({ "labels": add.labels }).to_string();
            Ok(send(
                at,
                "POST",
                &format!("/v1/runs/{}/labels", add.id),
                Some(Body::Json(body)),
                &[],
            )?
            .body)
        }
        Verb::Unlabel(off) => {
            send(
                at,
                "DELETE",
                &format!("/v1/runs/{}/labels/{}", off.id, escaped(&off.label)),
                None,
                &[],
            )?;
            Ok(off.label.clone())
        }
        Verb::Labels => Ok(get(at, "/v1/runs/labels", &[])?.body),
        Verb::Hide(which) => visibility(at, which, true),
        Verb::Unhide(which) => visibility(at, which, false),
    }
}

/// The console this command speaks to, and what it presents.
struct Console {
    origin: String,
    account: Option<String>,
    token: String,
}

/// A no, and what to exit with. `said` is the console's own sentence wherever there is one.
struct Refusal {
    said: String,
    code: u8,
}

impl Refusal {
    /// This machine could not do it: no token, no curl, an unwritable path.
    fn local(said: String) -> Self {
        Self {
            said,
            code: EXIT_OPERATIONAL,
        }
    }
}

/// The origin: the flag, then the environment, then the default, held to an `http` or `https`
/// address with no trailing slash so every lane joins onto it cleanly.
fn console(flag: Option<&str>, env: Option<String>) -> Result<String, String> {
    let named = flag.map(str::to_string).or(env);
    let Some(named) = named else {
        return Ok(DEFAULT_CONSOLE.to_string());
    };
    let trimmed = named.trim().trim_end_matches('/');
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(format!(
            "{named:?} is not a console address: it starts with http:// or https://"
        ));
    }
    Ok(trimmed.to_string())
}

/// The token: the environment first, so a machine with no app signed in still pushes, then the
/// one the app wrote when this device was paired.
///
/// Nothing here mints one. Signing in is the app's, and this reads what that left.
fn token() -> Result<String, Refusal> {
    if let Ok(from_env) = std::env::var(TOKEN_ENV)
        && !from_env.trim().is_empty()
    {
        return Ok(from_env.trim().to_string());
    }
    let path = device_token_path().ok_or_else(|| {
        Refusal::local("no HOME and no XDG_DATA_HOME, so there is nowhere a token could be".into())
    })?;
    match std::fs::read_to_string(&path) {
        Ok(held) if !held.trim().is_empty() => Ok(held.trim().to_string()),
        _ => Err(Refusal::local(format!(
            "this machine is not signed in to {DEFAULT_CONSOLE}. Sign in from Tormoni, or set ${TOKEN_ENV}"
        ))),
    }
}

/// Where the app keeps the token its device pairing collected: the same path
/// `tormoni_app::device` writes, spelled once here because the two binaries share no library.
fn device_token_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("tormoni/device/token"))
}

/// What a request carries, when it carries anything.
enum Body {
    /// A JSON document, as `application/json`.
    Json(String),
    /// A file's bytes, as `application/x-tar`. A path rather than the bytes, because curl's
    /// stdin is where the credential goes and an archive is too big to want in memory twice.
    Tar(PathBuf),
}

/// What came back: the status, and the body as text.
struct Answer {
    status: u16,
    body: String,
}

fn get(at: &Console, lane: &str, query: &[(&str, &str)]) -> Result<Answer, Refusal> {
    send(at, "GET", lane, None, query)
}

/// One request, and the console's answer read back by status.
///
/// A 2xx is the answer. Everything else is a [`Refusal`] carrying the server's own `detail`,
/// because that sentence is customer-facing text somebody maintains and a second one here would
/// drift from it.
fn send(
    at: &Console,
    method: &str,
    lane: &str,
    body: Option<Body>,
    query: &[(&str, &str)],
) -> Result<Answer, Refusal> {
    let answer = ask(at, method, lane, body, query).map_err(Refusal::local)?;
    if (200..300).contains(&answer.status) {
        return Ok(answer);
    }
    Err(Refusal {
        said: said(&answer),
        code: match answer.status {
            401 | 403 => EXIT_REFUSED,
            404 => EXIT_NOT_FOUND,
            409 => EXIT_CONFLICT,
            413 => EXIT_TOO_LARGE,
            _ => EXIT_SERVER,
        },
    })
}

/// The refusal as a person reads it: the server's `detail`, else its `title`, else the status
/// alone for a body that is not a problem document at all.
fn said(answer: &Answer) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(&answer.body).ok();
    let sentence = parsed.as_ref().and_then(|json| {
        ["detail", "title", "error"]
            .into_iter()
            .find_map(|key| json.get(key)?.as_str().map(str::to_string))
    });
    match sentence {
        Some(detail) => detail,
        None => format!("the console answered {}", answer.status),
    }
}

/// Runs one `curl`. The bearer goes in on stdin as a config file, so `ps` never sees it.
fn ask(
    at: &Console,
    method: &str,
    lane: &str,
    body: Option<Body>,
    query: &[(&str, &str)],
) -> Result<Answer, String> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--max-time",
        if matches!(body, Some(Body::Tar(_))) {
            PUSH_PATIENCE
        } else {
            PATIENCE
        },
        "--config",
        "-",
        "--header",
        "Accept: application/json",
        "--write-out",
        "\n%{http_code}",
    ]);
    // `--head`, never `--request HEAD`. The second one changes only the method, so curl still
    // expects the body the response advertises — and a HEAD correctly sends none, which curl
    // reports as `(18) transfer closed with N bytes remaining to read`. Every push asks HEAD
    // first, so spelling this wrong breaks every push and nothing else.
    if method == "HEAD" {
        cmd.arg("--head");
    } else {
        cmd.args(["--request", method]);
    }
    match &body {
        Some(Body::Json(_)) => {
            cmd.args(["--header", "Content-Type: application/json"]);
        }
        Some(Body::Tar(path)) => {
            cmd.args(["--header", "Content-Type: application/x-tar"]);
            cmd.arg("--data-binary").arg(format!("@{}", path.display()));
        }
        None => {}
    }
    cmd.arg(url(at, lane, query));

    // The credential and any JSON body both travel on stdin, as curl config lines: an argument
    // is on the process list and a temporary file is on the disk.
    let mut config = format!("header = \"Authorization: Bearer {}\"\n", at.token);
    if let Some(Body::Json(json)) = &body {
        config.push_str(&format!("data = {}\n", quoted(json)));
    }
    speak(cmd, &config)
}

/// The whole address: the origin, the lane, and the query the caller asked for with `?account=`
/// added where one was named.
fn url(at: &Console, lane: &str, query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(&str, &str)> = query.to_vec();
    if let Some(account) = &at.account {
        pairs.push(("account", account));
    }
    let mut out = format!("{}{lane}", at.origin);
    for (n, (key, value)) in pairs.iter().enumerate() {
        out.push(if n == 0 { '?' } else { '&' });
        out.push_str(key);
        out.push('=');
        out.push_str(&escaped(value));
    }
    out
}

/// A query value or a path segment with everything but the unreserved characters escaped, so a
/// label with a slash or a space cannot become part of the address.
fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A curl config value: quoted, with the two characters its parser reads escaped.
fn quoted(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Runs `cmd`, feeding `config` to its stdin, and splits the status off the tail.
fn speak(mut cmd: Command, config: &str) -> Result<Answer, String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(no_curl)?;
    if let Some(mut stdin) = child.stdin.take() {
        // A curl that died before reading is reported by its exit below, not by this write.
        let _ = stdin.write_all(config.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait for curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the console could not be reached: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, status) = text
        .rsplit_once('\n')
        .ok_or_else(|| "curl wrote no status".to_string())?;
    Ok(Answer {
        status: status
            .trim()
            .parse()
            .map_err(|_| format!("curl wrote {:?} where a status goes", status.trim()))?,
        body: body.to_string(),
    })
}

/// Why `curl` would not start. A host without it cannot reach the console at all, which is a
/// different thing from a console that is not answering, and reads as one.
fn no_curl(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "curl is not installed, and the console is reached through it".to_string()
    } else {
        format!("run curl: {e}")
    }
}

/// `cloud push`: ask whether the console has this run, and send the export only if it does not.
///
/// The ask is the point. An archive is tens of MiB and the answer is a header, so a run already
/// stored costs one round trip instead of the whole upload.
fn push_run(at: &Console, args: &PushArgs) -> Result<String, Refusal> {
    let (store, record) = crate::lifecycle::find_run(&args.key).map_err(Refusal::local)?;
    if !args.force {
        let head = ask(at, "HEAD", &format!("/v1/runs/{}", record.id), None, &[])
            .map_err(Refusal::local)?;
        if head.status == 200 {
            return Ok(format!("{} is already stored; nothing was sent", record.id));
        }
    }

    let scratch = tempdir().map_err(Refusal::local)?;
    let archive = store
        .export(&record.id, scratch.as_path())
        .map_err(|e| Refusal::local(format!("export {}: {e}", record.id)))?;

    let labels = args.label.join(",");
    let query: Vec<(&str, &str)> = if labels.is_empty() {
        Vec::new()
    } else {
        vec![("labels", labels.as_str())]
    };
    let answer = send(at, "POST", "/v1/runs", Some(Body::Tar(archive)), &query);
    let _ = std::fs::remove_dir_all(scratch.as_path());
    Ok(answer?.body)
}

/// A directory this process owns, for the archive a push uploads from.
fn tempdir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join(format!("tormoni-push-{}", std::process::id()));
    std::fs::create_dir_all(&base).map_err(|e| format!("create {}: {e}", base.display()))?;
    Ok(base)
}

/// `cloud ls`: one page, or every page when `--all` follows the cursor.
///
/// `next_after` being null is the only certain end: a page can be short without being the last.
fn list(at: &Console, args: &LsArgs) -> Result<String, Refusal> {
    let mut after = args.after.clone();
    let mut pages = Vec::new();
    loop {
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(label) = &args.label {
            query.push(("label", label));
        }
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }
        let answer = get(at, "/v1/runs", &query)?;
        let cursor = serde_json::from_str::<serde_json::Value>(&answer.body)
            .ok()
            .and_then(|json| json.get("next_after")?.as_str().map(str::to_string));
        pages.push(answer.body);
        after = cursor;
        if !args.all || after.is_none() {
            break;
        }
    }
    Ok(pages.join("\n"))
}

/// `cloud pull`: the archive back, to a file.
///
/// Written by curl rather than through this process, so a large one never sits in memory. A
/// directory takes `<id>.tar`, the shape `tormoni export --to` already has.
fn pull_archive(at: &Console, args: &PullArgs) -> Result<String, Refusal> {
    let out = if args.out.is_dir() {
        args.out.join(format!("{}.tar", args.id))
    } else {
        args.out.clone()
    };
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--max-time",
        PUSH_PATIENCE,
        "--config",
        "-",
        "--write-out",
        "\n%{http_code}",
        "--output",
    ])
    .arg(&out)
    .arg(url(at, &format!("/v1/runs/{}/archive", args.id), &[]));
    let config = format!("header = \"Authorization: Bearer {}\"\n", at.token);
    let answer = speak(cmd, &config).map_err(Refusal::local)?;
    if !(200..300).contains(&answer.status) {
        // curl wrote the refusal into the file, so read it back and then take the file away:
        // a `.tar` holding a problem document is worse than no file.
        let body = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        return Err(Refusal {
            said: said(&Answer {
                status: answer.status,
                body,
            }),
            code: match answer.status {
                401 | 403 => EXIT_REFUSED,
                404 => EXIT_NOT_FOUND,
                _ => EXIT_SERVER,
            },
        });
    }
    Ok(out.display().to_string())
}

/// `cloud hide` and `cloud unhide`: the owner's, and refused as a 403 for anybody else.
fn visibility(at: &Console, args: &VisibilityArgs, hidden: bool) -> Result<String, Refusal> {
    let body = serde_json::json!({ "hidden": hidden, "ids": args.ids }).to_string();
    Ok(send(
        at,
        "POST",
        "/v1/runs/visibility",
        Some(Body::Json(body)),
        &[],
    )?
    .body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(origin: &str, account: Option<&str>) -> Console {
        Console {
            origin: origin.to_string(),
            account: account.map(str::to_string),
            token: "tor_secret".to_string(),
        }
    }

    /// The origin resolves flag, then environment, then the default, and anything that is not an
    /// http address is refused rather than joined onto and sent somewhere.
    #[test]
    fn the_console_is_the_flag_then_the_environment_then_the_default() {
        assert_eq!(console(None, None).expect("a default"), DEFAULT_CONSOLE);
        assert_eq!(
            console(None, Some("http://localhost:3000".into())).expect("the environment"),
            "http://localhost:3000"
        );
        assert_eq!(
            console(Some("https://a.example/"), Some("http://b.example".into()))
                .expect("the flag outranks"),
            "https://a.example",
            "and a trailing slash is dropped, so a lane joins cleanly"
        );
        for bad in ["tormoni.ai", "ftp://x", "file:///etc"] {
            assert!(console(Some(bad), None).is_err(), "{bad} was accepted");
        }
    }

    /// `--account` is a query value on every lane, and a value that carries a `&` or a `/` is
    /// escaped rather than becoming part of the address.
    #[test]
    fn the_account_rides_the_query_and_a_value_cannot_leave_it() {
        assert_eq!(
            url(&at("https://c.example", None), "/v1/runs", &[]),
            "https://c.example/v1/runs"
        );
        assert_eq!(
            url(&at("https://c.example", Some("user_01")), "/v1/runs", &[]),
            "https://c.example/v1/runs?account=user_01"
        );
        assert_eq!(
            url(
                &at("https://c.example", Some("user_01")),
                "/v1/runs",
                &[("label", "a b&c")]
            ),
            "https://c.example/v1/runs?label=a%20b%26c&account=user_01"
        );
    }

    /// **The refusal a person sees is the console's own sentence**, and each status exits with
    /// its own code so a script tells them apart without reading English.
    #[test]
    fn a_refusal_carries_the_servers_words_and_its_own_exit_code() {
        let problem = |status, detail: &str| Answer {
            status,
            body: serde_json::json!({
                "type": "/errors/payload-too-large",
                "title": "payload too large",
                "status": status,
                "detail": detail,
            })
            .to_string(),
        };
        assert_eq!(
            said(&problem(
                413,
                "The upload is larger than this lane accepts."
            )),
            "The upload is larger than this lane accepts."
        );
        // A body that is not a problem document at all still says something useful.
        assert_eq!(
            said(&Answer {
                status: 502,
                body: "<html>".into()
            }),
            "the console answered 502"
        );
        // A problem with no `detail` falls back to its title rather than to nothing.
        assert_eq!(
            said(&Answer {
                status: 403,
                body: serde_json::json!({ "title": "forbidden" }).to_string()
            }),
            "forbidden"
        );
    }

    /// The bearer is a curl config line and never an argument: a process list is public, and
    /// `tormoni cloud` runs on machines other people can read.
    #[test]
    fn the_bearer_is_never_in_the_process_list() {
        let mut cmd = Command::new("curl");
        cmd.args(["--config", "-"]).arg(url(
            &at("https://c.example", Some("user_01")),
            "/v1/runs",
            &[],
        ));
        let argv = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!argv.contains("tor_secret"), "{argv}");
        assert_eq!(
            quoted(r#"{"labels":["a\"b"]}"#),
            r#""{\"labels\":[\"a\\\"b\"]}""#,
            "a body with quotes survives curl's config parser"
        );
    }

    /// The token is the environment's, else the file the app's device pairing wrote. Nothing
    /// here mints one, so a machine that never signed in is told where to.
    #[test]
    fn the_token_path_is_the_one_the_app_writes() {
        let path = device_token_path().expect("a home on this host");
        assert!(path.ends_with("tormoni/device/token"), "{}", path.display());
    }
}
