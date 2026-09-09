//! The account this window is signed in to, and the one place a sign-in happens.
//!
//! - **Nothing in this window needs one.** Every sandbox it starts, lists and shows is local, and
//!   no part of that asks who you are. Signing in is one block on Settings, never in front of
//!   the notebook: there is no screen a signed-out person cannot reach.
//! - **The token is the account.** The console mints a `tor_` token on its Keys page and its API
//!   answers to that bearer alone, so signing in is pasting one and [`begin`] asking
//!   `/v1/account` whose it is. Nothing else is held: once the console has answered, the window
//!   keeps the identity and the [`Token`] is wiped.
//! - **The console is one origin.** `--console`, else `$TORMONI_CONSOLE`, else the product's own
//!   ([`console`]): where a token is minted, the account read, and Manage and Upgrade opened.
//! - **`curl` carries the request**, handed the bearer on its stdin rather than its command
//!   line, which `ps` shows. The tree has no HTTP client; a host without `curl` gets a typed
//!   error from the one press that needs it.
//! - **Nothing is written to disk.** A credential belongs in the platform's own store, and this
//!   build has none to put one in.

use std::process::{Command, Stdio};

use zeroize::Zeroize;

/// Who this window is signed in as.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Account {
    /// Nobody, which is every launch so far.
    #[default]
    SignedOut,
    /// A token is being pasted, and this is as much of it as has arrived.
    Entering(Token),
    /// The console is being asked whose the token is, and nothing here is pressable until it
    /// answers.
    SigningIn,
    /// Signed in, as this identity.
    SignedIn(Identity),
}

/// A `tor_` token as pasted: wiped when dropped, and printed as nothing.
#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct Token(String);

impl Token {
    /// What has been pasted, which is what the field shows.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The token as one word: `tor_`, then the base64url the console mints, so that nothing a
    /// header or curl's config would read as its own can reach either.
    fn checked(&self) -> Result<&str, String> {
        let raw = self.0.trim();
        if !raw.starts_with("tor_") {
            return Err(
                "a token starts with tor_: mint one on the console's Keys page".to_string(),
            );
        }
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if !raw.chars().all(word) {
            return Err("a token is one word of letters, digits, _ and -".to_string());
        }
        Ok(raw)
    }
}

impl From<String> for Token {
    fn from(pasted: String) -> Self {
        Self(pasted)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// What `/v1/account` answers with: the handle the account is reached by, and what the identity
/// provider calls the person, when it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) handle: String,
    pub(crate) display_name: Option<String>,
}

impl Account {
    /// What the block is headed by: the person's name, else the handle, else the product's name
    /// for the account there is not yet.
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::SignedOut | Self::Entering(_) | Self::SigningIn => "Tormoni account",
            Self::SignedIn(identity) => {
                identity.display_name.as_deref().unwrap_or(&identity.handle)
            }
        }
    }

    /// The quieter line under the title: the handle, or where the sign-in stands and what it
    /// wants from `console`.
    pub(crate) fn line(&self, console: &str) -> String {
        match self {
            Self::SignedOut => "Not connected".to_string(),
            Self::Entering(_) => format!(
                "Paste a token from {}",
                without_scheme(&Page::Keys.url(console))
            ),
            Self::SigningIn => "Signing in…".to_string(),
            Self::SignedIn(identity) => format!("@{}", identity.handle),
        }
    }
}

/// The console this build signs in to when nothing names another.
pub(crate) const DEFAULT: &str = "https://tormoni.ai";

/// The environment variable that names the console, under the flag.
pub(crate) const ENV: &str = "TORMONI_CONSOLE";

/// The console's origin: the flag, else the environment, else [`DEFAULT`], held to an `http` or
/// `https` address with any trailing slash dropped, since every page is joined onto it.
pub(crate) fn console(flag: Option<&str>, env: Option<String>) -> Result<String, String> {
    let Some(asked) = flag.map(str::to_string).or(env) else {
        return Ok(DEFAULT.to_string());
    };
    let origin = asked.trim().trim_end_matches('/');
    if !(origin.starts_with("http://") || origin.starts_with("https://")) {
        return Err(format!(
            "the console is an http:// or https:// address, not {asked:?}"
        ));
    }
    Ok(origin.to_string())
}

/// An address as a person reads it, without the scheme.
fn without_scheme(url: &str) -> &str {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
}

/// A page of the console the account block opens: Keys, where a token is minted; the account,
/// which Manage goes to; and the plans, which Upgrade goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Keys,
    Account,
    Plans,
}

impl Page {
    /// The page's address on `console`.
    pub(crate) fn url(self, console: &str) -> String {
        let path = match self {
            Self::Keys => "/keys",
            Self::Account => "/account",
            Self::Plans => "/plans",
        };
        format!("{console}{path}")
    }
}

/// Opens `page` of `console` in the browser, answering with the line the operator reads.
pub(crate) fn open(console: &str, page: Page) -> Result<String, String> {
    let url = page.url(console);
    let mut cmd = browser(&url);
    let opener = cmd.get_program().to_string_lossy().into_owned();
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("run {opener}: {e}"))?;
    crate::cli::reap(child);
    Ok(format!("opened {url} in the browser"))
}

/// The platform's own opener, handed `url`: `open` on macOS, `xdg-open` on every other host.
fn browser(url: &str) -> Command {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut cmd = Command::new(opener);
    cmd.arg(url);
    cmd
}

/// The console's lane that says whose a token is.
const ACCOUNT_LANE: &str = "/v1/account";

/// How long the console gets to answer, in seconds: one read behind one bearer check.
const PATIENCE: &str = "10";

/// Signs in to `console` with `token`: asks its account lane whose it is.
pub(crate) fn begin(console: &str, token: &Token) -> Result<Identity, String> {
    let (mut cmd, config) = request(console, token)?;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("run curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        // A curl that died before reading is reported by its exit below, not by this write.
        let _ = stdin.write_all(config.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait for curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    answer(&String::from_utf8_lossy(&out.stdout))
}

/// The `curl` that asks `console` whose `token` is, and the config it reads from its stdin,
/// which is where the bearer travels: nothing in the command's own arguments is secret.
fn request(console: &str, token: &Token) -> Result<(Command, String), String> {
    let token = token.checked()?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--max-time",
        PATIENCE,
        "--config",
        "-",
        "--header",
        "Accept: application/json",
        "--write-out",
        "\n%{http_code}",
    ])
    .arg(format!("{console}{ACCOUNT_LANE}"));
    Ok((cmd, format!("header = \"Authorization: Bearer {token}\"\n")))
}

/// What curl wrote: the body, then the status on a line of its own, read back as the identity
/// or the reason there is none.
fn answer(out: &str) -> Result<Identity, String> {
    let (body, status) = out
        .rsplit_once('\n')
        .ok_or_else(|| "curl wrote no status".to_string())?;
    match status.trim() {
        "200" => identity(body),
        "401" | "403" => Err(
            "the console refused the token: mint one on its Keys page and paste it whole"
                .to_string(),
        ),
        other => Err(format!("the console answered {other}{}", said(body))),
    }
}

/// The identity in the account lane's answer.
fn identity(body: &str) -> Result<Identity, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("the account is not JSON: {e}"))?;
    let handle = json
        .get("handle")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "the account names no handle".to_string())?
        .to_string();
    let display_name = json
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(Identity {
        handle,
        display_name,
    })
}

/// What an error body says, in the console's two spellings, or nothing.
fn said(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            ["error", "detail"]
                .into_iter()
                .find_map(|key| json.get(key)?.as_str().map(|s| format!(": {s}")))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each state reads as itself on the block: the product's name over where a sign-in stands
    /// until one is in, then the person over the handle, or the handle over itself when the
    /// provider gave no name.
    #[test]
    fn the_block_reads_the_state_it_is_in() {
        let console = "http://localhost:3000";
        assert_eq!(Account::default(), Account::SignedOut);
        assert_eq!(Account::SignedOut.title(), "Tormoni account");
        assert_eq!(Account::SignedOut.line(console), "Not connected");
        let entering = Account::Entering(Token::default());
        assert_eq!(entering.title(), "Tormoni account");
        assert_eq!(
            entering.line(console),
            "Paste a token from localhost:3000/keys"
        );
        assert_eq!(Account::SigningIn.line(console), "Signing in…");
        let named = Account::SignedIn(Identity {
            handle: "someone".to_string(),
            display_name: Some("Someone Else".to_string()),
        });
        assert_eq!(named.title(), "Someone Else");
        assert_eq!(named.line(console), "@someone");
        let unnamed = Account::SignedIn(Identity {
            handle: "someone".to_string(),
            display_name: None,
        });
        assert_eq!(unnamed.title(), "someone");
    }

    /// The console is the flag, else the environment, else the product's own; a trailing slash
    /// is dropped so a page joins cleanly, and anything but an http(s) address is refused.
    #[test]
    fn the_console_is_the_flag_then_the_environment_then_the_default() {
        assert_eq!(console(None, None).as_deref(), Ok(DEFAULT));
        assert_eq!(
            console(None, Some("http://localhost:3000/".to_string())).as_deref(),
            Ok("http://localhost:3000")
        );
        assert_eq!(
            console(
                Some("https://dev.tormoni.ai"),
                Some("http://localhost:3000".to_string())
            )
            .as_deref(),
            Ok("https://dev.tormoni.ai")
        );
        let why = console(Some("localhost:3000"), None).expect_err("no scheme");
        assert!(why.contains("http://"), "{why}");
    }

    /// Manage and Upgrade open the console's account and plans pages, and Sign in its Keys
    /// page, each under the one origin the window was given.
    #[test]
    fn each_page_is_on_the_console() {
        assert_eq!(
            Page::Keys.url("http://localhost:3000"),
            "http://localhost:3000/keys"
        );
        assert_eq!(Page::Account.url(DEFAULT), "https://tormoni.ai/account");
        assert_eq!(Page::Plans.url(DEFAULT), "https://tormoni.ai/plans");
    }

    /// The opener is the platform's own, handed the address and nothing else: a flag before it
    /// would be read as the thing to open.
    #[test]
    fn the_browser_is_the_platforms_opener_handed_the_address() {
        let cmd = browser("https://tormoni.ai/account");
        let expected = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        assert_eq!(cmd.get_program(), expected);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["https://tormoni.ai/account"]);
    }

    /// A token is one `tor_` word in the console's own alphabet; whitespace around it is what a
    /// paste brings and is dropped, and anything else in it is refused before curl sees it.
    #[test]
    fn a_token_is_one_word_that_starts_with_tor() {
        let ok = Token::from("  tor_abc-XYZ_09  ".to_string());
        assert_eq!(ok.checked().as_deref(), Ok("tor_abc-XYZ_09"));
        let why = Token::from("abc".to_string()).expect_refused();
        assert!(why.contains("tor_"), "{why}");
        let why = Token::from("tor_abc\"\nurl = \"http://evil\"".to_string()).expect_refused();
        assert!(why.contains("one word"), "{why}");
    }

    impl Token {
        fn expect_refused(&self) -> String {
            self.checked().expect_err("refused")
        }
    }

    /// The bearer travels on curl's stdin and nowhere in its arguments, which is what keeps it
    /// off the process list; the address is the console's account lane.
    #[test]
    fn the_request_carries_the_token_on_stdin_and_never_in_argv() {
        let token = Token::from("tor_secret".to_string());
        let (cmd, config) = request("http://localhost:3000", &token).expect("a request");
        assert_eq!(cmd.get_program(), "curl");
        let argv = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!argv.contains("tor_secret"), "{argv}");
        assert!(argv.contains("--config -"), "{argv}");
        assert!(argv.ends_with("http://localhost:3000/v1/account"), "{argv}");
        assert_eq!(config, "header = \"Authorization: Bearer tor_secret\"\n");
    }

    /// A token is never printed: the state it sits in is `Debug`, and a log line of that state
    /// would otherwise carry the credential.
    #[test]
    fn a_token_prints_as_nothing() {
        let shown = format!(
            "{:?}",
            Account::Entering(Token::from("tor_secret".to_string()))
        );
        assert!(!shown.contains("secret"), "{shown}");
    }

    /// The console's answer becomes the identity, with or without a display name; a refusal
    /// and a stray status each become a line that says what to do, and an error body's own
    /// words are carried along.
    #[test]
    fn the_answer_becomes_an_identity_or_a_reason() {
        let named = answer(
            "{\"account_id\":\"acc_1\",\"handle\":\"kendrick\",\"display_name\":\"Kendrick L\"}\n200",
        )
        .expect("signed in");
        assert_eq!(named.handle, "kendrick");
        assert_eq!(named.display_name.as_deref(), Some("Kendrick L"));
        let unnamed =
            answer("{\"handle\":\"kendrick\",\"display_name\":null}\n200").expect("signed in");
        assert_eq!(unnamed.display_name, None);

        let refused =
            answer("{\"error\":\"a live tor_ API token is required\"}\n401").expect_err("refused");
        assert!(refused.contains("refused"), "{refused}");
        let down = answer("{\"error\":\"database unavailable\"}\n503").expect_err("down");
        assert_eq!(down, "the console answered 503: database unavailable");
        let odd = answer("<html>\n404").expect_err("not the console");
        assert_eq!(odd, "the console answered 404");
        let noise = answer("nonsense\n200").expect_err("not JSON");
        assert!(noise.contains("not JSON"), "{noise}");
    }

    /// A sign-in end to end: curl carries the token to a console this test stands up on a
    /// loopback port, which answers as the account lane does, and the identity comes back.
    /// Skipped, and says so, where there is no curl to carry it.
    #[test]
    fn a_sign_in_reaches_the_console_and_reads_the_account_back() {
        if !on_path("curl") {
            eprintln!("skipped: no curl on PATH, so nothing can carry the request");
            return;
        }
        let console = FakeConsole::answering(
            200,
            "{\"account_id\":\"acc_1\",\"handle\":\"kendrick\",\"display_name\":null}",
        );
        let identity =
            begin(&console.origin(), &Token::from("tor_test".to_string())).expect("signed in");
        assert_eq!(identity.handle, "kendrick");
        let seen = console.request();
        assert!(seen.starts_with("GET /v1/account HTTP/1.1"), "{seen}");
        assert!(seen.contains("Authorization: Bearer tor_test"), "{seen}");

        let refusing =
            FakeConsole::answering(401, "{\"error\":\"a live tor_ API token is required\"}");
        let why =
            begin(&refusing.origin(), &Token::from("tor_nope".to_string())).expect_err("refused");
        assert!(why.contains("refused"), "{why}");
    }

    /// Whether `program` is somewhere on `PATH`.
    fn on_path(program: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    }

    /// One HTTP answer on a loopback port, and the request that came for it.
    struct FakeConsole {
        port: u16,
        seen: std::sync::mpsc::Receiver<String>,
    }

    impl FakeConsole {
        fn answering(status: u16, body: &'static str) -> Self {
            use std::io::{BufRead, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
            let port = listener.local_addr().expect("bound").port();
            let (tx, seen) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let (stream, _) = listener.accept().expect("one connection");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .expect("a timeout");
                let mut reader = std::io::BufReader::new(&stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).expect("a line") == 0 || line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                let mut writer = &stream;
                write!(
                    writer,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("answered");
                writer.flush().expect("flushed");
                tx.send(request).expect("the request is wanted");
            });
            Self { port, seen }
        }

        fn origin(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        fn request(&self) -> String {
            self.seen
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("one request")
        }
    }
}
