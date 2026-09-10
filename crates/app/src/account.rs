//! The account this window is signed in to, and the one place a sign-in happens.
//!
//! - **Nothing in this window needs one.** Every sandbox it starts, lists and shows is local, and
//!   no part of that asks who you are. Signing in is one block on Settings, never in front of
//!   the notebook: there is no screen a signed-out person cannot reach.
//! - **Signing in pairs a device, and needs the console.** The app makes a key
//!   ([`crate::device`]), opens [`connect_url`] in the browser, and polls [`claim`] until the
//!   person approves it there; the console then hands over a `tor_` token, which [`begin`] spends
//!   on `/v1/account` to learn whose it is. There is no token to copy and no offline path: with
//!   the console unreachable the block can only say so.
//! - **The console is one origin.** `--console`, else `$TORMONI_CONSOLE`, else the product's own
//!   ([`console`]): where a token is minted, the account read, and Manage and Upgrade opened.
//! - **`curl` carries the request**, handed the bearer on its stdin rather than its command
//!   line, which `ps` shows. The tree has no HTTP client; a host without `curl` gets a typed
//!   error from the one press that needs it.
//! - **The token is a file at `0600`, not a keychain.** This build has no credential store to
//!   put one in, so [`crate::device`] keeps it beside the key that claimed it and Sign out
//!   destroys both.

use std::process::{Command, Stdio};

use zeroize::Zeroize;

/// Who this window is signed in as.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Account {
    /// Nobody, which is every launch so far.
    #[default]
    SignedOut,
    /// A device key is waiting to be approved: the browser is open on the console and the app
    /// is polling. Carries what a person needs to check the page against.
    Pairing(Pairing),
    /// Signed in, as this identity.
    SignedIn(Identity),
}

/// What the block shows while the console has not yet been told to approve this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pairing {
    /// This machine, as the page names it.
    pub(crate) device: String,
    /// The public half the page was opened with.
    pub(crate) line: String,
    /// What a person compares against the page before pressing Connect.
    pub(crate) fingerprint: String,
    /// The last second a claim signed at, so the next one never repeats it.
    pub(crate) issued_at: i64,
    /// When Sign in was pressed, which is what [`Pairing::gave_up`] measures from.
    pub(crate) started_ms: u64,
}

/// How long a device waits to be approved before the window stops asking. The console lets an
/// approval nobody claims lapse after ten minutes, so giving up sooner strands nothing.
const GIVE_UP_AFTER_MS: u64 = 5 * 60 * 1000;

impl Pairing {
    /// Whether this pairing has waited longer than anyone is going to approve it in.
    pub(crate) fn gave_up(&self) -> bool {
        tormoni_record::now_ms().saturating_sub(self.started_ms) > GIVE_UP_AFTER_MS
    }
}

/// A `tor_` token: wiped when dropped, and printed as nothing.
#[derive(Clone, PartialEq, Eq, Default)]
pub(crate) struct Token(String);

impl Token {
    /// The token as one word: `tor_`, then the base64url the console mints, so that nothing a
    /// header or curl's config would read as its own can reach either.
    fn checked(&self) -> Result<&str, String> {
        let raw = self.0.trim();
        if !raw.starts_with("tor_") {
            return Err("the console answered with something that is not a tor_ token".to_string());
        }
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if !raw.chars().all(word) {
            return Err("a token is one word of letters, digits, _ and -".to_string());
        }
        Ok(raw)
    }
}

impl From<String> for Token {
    fn from(minted: String) -> Self {
        Self(minted)
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

/// What `/v1/account` answers with: the address the account is named by, and what the identity
/// provider calls the person, when it says.
///
/// **The address is the identity here.** The console has no handle: its own `account_id` is an
/// opaque provider id (`user_01M0T…`), which names nobody on a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) email: String,
    pub(crate) display_name: Option<String>,
}

impl Account {
    /// What the block is headed by: the person's name, else the address, else the product's name
    /// for the account there is not yet.
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::SignedOut | Self::Pairing(_) => "Tormoni account",
            Self::SignedIn(identity) => identity.display_name.as_deref().unwrap_or(&identity.email),
        }
    }

    /// The quieter line under the title: the address, or where the sign-in stands and what it
    /// wants from `console`.
    pub(crate) fn line(&self, console: &str) -> String {
        match self {
            Self::SignedOut => "Not connected".to_string(),
            Self::Pairing(_) => format!(
                "Waiting for approval at {}/connect",
                without_scheme(console)
            ),
            Self::SignedIn(identity) => identity.email.clone(),
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

/// Where this device's key and token live.
pub(crate) fn dir() -> Result<std::path::PathBuf, String> {
    crate::device::dir()
}

/// What the connect page calls this machine: its hostname, else a plain word, since the name is
/// only there for a person to recognise their own laptop in a list.
pub(crate) fn device_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this machine".to_string())
}

/// Keeps the token this device claimed, then spends it on the account lane, so the identity the
/// window shows is one the console answered for the token it just handed over.
///
/// Kept before it is read with: the console hands a key its token once, so a token dropped
/// because the read failed could never be asked for again.
pub(crate) fn finish(
    console: &str,
    dir: &std::path::Path,
    token: Token,
) -> Result<Identity, String> {
    crate::device::save_token(dir, token.checked()?)?;
    begin(console, &token)
}

/// Opens `url` in the browser, answering with the line the operator reads.
pub(crate) fn open_url(url: &str) -> Result<String, String> {
    let mut cmd = browser(url);
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

/// Opens `page` of `console` in the browser.
pub(crate) fn open(console: &str, page: Page) -> Result<String, String> {
    open_url(&page.url(console))
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

/// The lane a device gives its token up on. It names no device and carries no body: the bearer
/// IS the device, so signing out is one request with nothing in it.
const SIGN_OUT_LANE: &str = "/v1/device";

/// The lane a paired device collects its token from, which takes no bearer because the caller
/// has none yet. `contract/wire-contract.json` names it `device_claim.lane`.
const CLAIM_LANE: &str = "/v1/device/claim";

/// The page a person approves this device on, which takes the device's name and public half.
///
/// Not a [`Page`]: those are plain paths this joins onto an origin, and this one carries a
/// query, so it is built here rather than making every page take arguments it has no use for.
pub(crate) fn connect_url(console: &str, device: &str, line: &str) -> String {
    format!(
        "{console}/connect?name={}&key={}",
        encoded(device),
        encoded(line)
    )
}

/// A query value with everything but the unreserved characters escaped.
///
/// **`+` and `/` above all.** A public key is base64 and carries both; the console re-serializes
/// the query across the sign-in round trip, and a raw `+` comes back as a space, which is a key
/// that no longer parses.
fn encoded(value: &str) -> String {
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

/// How long the console gets to answer, in seconds: one read behind one bearer check.
const PATIENCE: &str = "10";

/// Signs in to `console` with `token`: asks its account lane whose it is.
pub(crate) fn begin(console: &str, token: &Token) -> Result<Identity, String> {
    let (cmd, config) = request(console, ACCOUNT_LANE, "GET", token)?;
    answer(&spoken(cmd, config)?)
}

/// Gives `token` back to `console`, which revokes it and the device it was minted for.
///
/// The caller has already taken it off this disk, and that order is the point: nothing here
/// decides whether this machine keeps a credential, so a console that cannot be reached leaves
/// a row for a key nobody holds rather than a key nobody revoked.
pub(crate) fn retire(console: &str, token: String) -> Result<(), String> {
    let token = Token::from(token);
    let (cmd, config) = request(console, SIGN_OUT_LANE, "DELETE", &token)?;
    handed_back(&spoken(cmd, config)?)
}

/// What the sign-out lane answered, by status.
fn handed_back(out: &str) -> Result<(), String> {
    let (body, status) = out
        .rsplit_once('\n')
        .ok_or_else(|| "curl wrote no status".to_string())?;
    match status.trim() {
        // A token the console has already stopped honouring is the state a sign-out asks for,
        // so 401 is this request's own answer arriving a second time, not a failure.
        "204" | "401" => Ok(()),
        other => Err(format!("it answered {other}{}", said(body))),
    }
}

/// Runs `cmd`, feeding `config` to its stdin, and hands back what it wrote.
fn spoken(mut cmd: Command, config: String) -> Result<String, String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(no_curl)?;
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
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// How a claim went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Claimed {
    /// Approved: the console handed over the token, and will not do so again for this key.
    Token(Token),
    /// Not approved yet, nor is a key the console has never seen. Ask again in this many seconds.
    Pending(u64),
    /// Stop, and say this. A refusal, a spent key, or a console that could not be reached.
    Refused(String),
}

/// One claim: the second it signed at, so the next never repeats it, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Claim {
    pub(crate) issued_at: i64,
    pub(crate) outcome: Claimed,
}

/// How often to ask again while a device waits for approval.
const POLL_EVERY: u64 = 2;

/// Asks `console` whether this device has been approved, signing with the key in `dir`.
///
/// `after` is the second the last claim signed at. ed25519 is deterministic, so a second claim at
/// the same second is the same bytes, which the console reads as a replay and refuses; this never
/// signs twice for one second.
pub(crate) fn claim(console: &str, dir: &std::path::Path, after: i64) -> Claim {
    let issued_at = now_seconds().max(after.saturating_add(1));
    let outcome = match sign_claim(dir, issued_at) {
        Ok(body) => match post(console, &body) {
            Ok(out) => claimed(&out),
            Err(why) => Claimed::Refused(why),
        },
        Err(why) => Claimed::Refused(why),
    };
    Claim { issued_at, outcome }
}

/// The claim body: the public half, the second, and the signature over the console's template.
fn sign_claim(dir: &std::path::Path, issued_at: i64) -> Result<String, String> {
    let key = crate::device::load(dir)?;
    let message = crate::device::claim_message(key.public().fingerprint(), issued_at);
    Ok(serde_json::json!({
        "key": key.public().line(),
        "issued_at": issued_at,
        "signature": key.sign(&message),
    })
    .to_string())
}

/// Posts `body` to the claim lane. Nothing here is secret, so it travels as a `--data` argument
/// would be visible: it is a public key and a signature over a public string.
fn post(console: &str, body: &str) -> Result<String, String> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--max-time",
        PATIENCE,
        "--header",
        "Content-Type: application/json",
        "--header",
        "Accept: application/json",
        "--data-binary",
        "@-",
        "--write-out",
        "\n%{http_code}",
    ])
    .arg(format!("{console}{CLAIM_LANE}"));
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(no_curl)?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait for curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the console at {console} could not be reached: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

/// What the claim lane answered, by status: each one its own outcome.
fn claimed(out: &str) -> Claimed {
    let Some((body, status)) = out.rsplit_once('\n') else {
        return Claimed::Refused("curl wrote no status".to_string());
    };
    match status.trim() {
        "200" => match serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|json| json.get("token")?.as_str().map(str::to_string))
        {
            Some(token) => Claimed::Token(Token::from(token)),
            None => {
                Claimed::Refused("the console approved this device but sent no token".to_string())
            }
        },
        "202" => Claimed::Pending(POLL_EVERY),
        "410" => Claimed::Refused(
            "this device key already collected its token; press Sign in again for a new one"
                .to_string(),
        ),
        "429" => Claimed::Pending(retry_after(body).unwrap_or(POLL_EVERY)),
        "400" => Claimed::Refused(format!("the console refused this device{}", said(body))),
        other => Claimed::Refused(format!("the console answered {other}{}", said(body))),
    }
}

/// How long a rate-limited console asked to be left alone, from the body it says it in.
fn retry_after(body: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("retry_after")?
        .as_u64()
}

/// This second, as the claim counts them.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// The `curl` that reaches `lane` as `token`, and the config it reads from its stdin, which is
/// where the bearer travels: nothing in the command's own arguments is secret.
fn request(
    console: &str,
    lane: &str,
    method: &str,
    token: &Token,
) -> Result<(Command, String), String> {
    let token = token.checked()?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--max-time",
        PATIENCE,
        "--request",
        method,
        "--config",
        "-",
        "--header",
        "Accept: application/json",
        "--write-out",
        "\n%{http_code}",
    ])
    .arg(format!("{console}{lane}"));
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
            "the console refused this device's token: it may have been revoked on its Keys page"
                .to_string(),
        ),
        other => Err(format!("the console answered {other}{}", said(body))),
    }
}

/// The identity in the account lane's answer.
fn identity(body: &str) -> Result<Identity, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("the account is not JSON: {e}"))?;
    let email = json
        .get("email")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "the account names no address".to_string())?
        .to_string();
    let display_name = json
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(Identity {
        email,
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
#[allow(clippy::panic, reason = "the live pairing test says why it stopped")]
mod tests {
    use super::*;

    /// The whole pairing against a console that is actually running: make a key, wait for it to
    /// be approved, claim the token, and spend it on the account lane.
    ///
    /// By hand, because approving is a person pressing Connect (or `make device-pair` in the
    /// cloud checkout). Prints the key line so the approver has it.
    #[test]
    #[ignore = "pairs with a live console: set $TORMONI_CONSOLE and approve the key it prints"]
    fn pairs_against_a_live_console() {
        let console = console(None, std::env::var(ENV).ok()).expect("a console");
        let scratch = tormoni_test_support::ScratchDir::created("live-pairing");
        let key = crate::device::create(scratch.path()).expect("a key");
        println!("KEY={}", key.public().line());
        println!("FINGERPRINT={}", key.public().fingerprint());
        println!(
            "URL={}",
            connect_url(&console, &device_name(), key.public().line())
        );

        let mut issued_at = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let token = loop {
            assert!(
                std::time::Instant::now() < deadline,
                "nobody approved the key"
            );
            let claim = claim(&console, scratch.path(), issued_at);
            issued_at = claim.issued_at;
            match claim.outcome {
                Claimed::Pending(after) => {
                    println!("pending at {issued_at}");
                    std::thread::sleep(std::time::Duration::from_secs(after));
                }
                Claimed::Token(token) => break token,
                Claimed::Refused(why) => panic!("{why}"),
            }
        };
        let identity = finish(&console, scratch.path(), token).expect("the token reads an account");
        println!(
            "SIGNED-IN email={} name={:?}",
            identity.email, identity.display_name
        );
    }

    /// Why a claim stopped the polling, or what it was instead, so an assertion on the reason
    /// names what it actually got.
    fn refusal(outcome: Claimed) -> String {
        match outcome {
            Claimed::Refused(why) => why,
            other => format!("not a refusal: {other:?}"),
        }
    }

    /// A pairing that has not been approved, for the states below.
    fn waiting(started_ms: u64) -> Pairing {
        Pairing {
            device: "a laptop".to_string(),
            line: "ssh-ed25519 AAAA".to_string(),
            fingerprint: "SHA256:abc".to_string(),
            issued_at: 0,
            started_ms,
        }
    }

    /// Each state reads as itself on the block: the product's name while nobody is signed in or
    /// a device waits to be approved, then the person over the address the account is named by.
    #[test]
    fn the_block_reads_the_state_it_is_in() {
        let console = "http://localhost:3000";
        assert_eq!(Account::default(), Account::SignedOut);
        assert_eq!(Account::SignedOut.title(), "Tormoni account");
        assert_eq!(Account::SignedOut.line(console), "Not connected");

        let pairing = Account::Pairing(waiting(tormoni_record::now_ms()));
        assert_eq!(pairing.title(), "Tormoni account");
        assert_eq!(
            pairing.line(console),
            "Waiting for approval at localhost:3000/connect"
        );

        let named = Account::SignedIn(Identity {
            email: "someone@example.com".to_string(),
            display_name: Some("Someone Else".to_string()),
        });
        assert_eq!(named.title(), "Someone Else");
        assert_eq!(named.line(console), "someone@example.com");
        let unnamed = Account::SignedIn(Identity {
            email: "someone@example.com".to_string(),
            display_name: None,
        });
        // With no name from the provider, the address heads the block: it is the only thing the
        // console gave that names a person.
        assert_eq!(unnamed.title(), "someone@example.com");
    }

    /// A device nobody approves is given up on, so the block does not wait for a page whose
    /// browser tab was closed an hour ago.
    #[test]
    fn a_pairing_nobody_approves_is_given_up_on() {
        assert!(!waiting(tormoni_record::now_ms()).gave_up());
        let stale = waiting(tormoni_record::now_ms() - GIVE_UP_AFTER_MS - 1);
        assert!(stale.gave_up());
    }

    /// The page carries the device and its key, and both are escaped: a public key is base64,
    /// which holds `+` and `/`, and a raw `+` comes back from a login round trip as a space.
    #[test]
    fn the_connect_page_escapes_the_key_and_the_name() {
        let url = connect_url("http://localhost:3000", "a laptop", "ssh-ed25519 AAAA+b/c=");
        assert_eq!(
            url,
            "http://localhost:3000/connect?name=a%20laptop&key=ssh-ed25519%20AAAA%2Bb%2Fc%3D"
        );
        assert!(!url.contains('+'), "a raw + would return as a space: {url}");
        assert!(
            !url.trim_start_matches("http://").contains('/') || url.matches("%2F").count() == 1,
            "the key's own slash must be escaped: {url}"
        );
    }

    /// Every status the claim lane answers with becomes its own outcome, because the app does a
    /// different thing for each: keep waiting, sign in, or stop and say why.
    #[test]
    fn each_claim_status_becomes_its_own_outcome() {
        assert_eq!(
            claimed("{\"token\":\"tor_abc\"}\n200"),
            Claimed::Token(Token::from("tor_abc".to_string()))
        );
        assert_eq!(
            claimed("{\"status\":\"pending\"}\n202"),
            Claimed::Pending(POLL_EVERY)
        );
        assert_eq!(claimed("{\"retry_after\":30}\n429"), Claimed::Pending(30));
        assert_eq!(claimed("{}\n429"), Claimed::Pending(POLL_EVERY));

        let spent = refusal(claimed("{\"detail\":\"gone\"}\n410"));
        assert!(spent.contains("already collected"), "{spent}");
        let bad = refusal(claimed("{\"detail\":\"stale issued_at\"}\n400"));
        assert!(bad.contains("stale issued_at"), "{bad}");
        let odd = refusal(claimed("<html>\n503"));
        assert!(odd.contains("503"), "{odd}");
        let empty = refusal(claimed("{}\n200"));
        assert!(empty.contains("no token"), "{empty}");
    }

    /// Two claims never share a second: ed25519 is deterministic, so the same second would be
    /// the same signature, which the console reads as a replay rather than a second ask.
    #[test]
    fn two_claims_never_share_a_second() {
        let scratch = tormoni_test_support::ScratchDir::created("claim-seconds");
        crate::device::create(scratch.path()).expect("a key");
        // The console is not there, so each claim is refused; the second it signed at is what
        // this checks, and that is chosen before anything is sent.
        let first = claim("http://127.0.0.1:1", scratch.path(), 0);
        let second = claim("http://127.0.0.1:1", scratch.path(), first.issued_at);
        assert!(
            second.issued_at > first.issued_at,
            "{} then {}",
            first.issued_at,
            second.issued_at
        );
        let far = claim("http://127.0.0.1:1", scratch.path(), 9_999_999_999);
        assert_eq!(
            far.issued_at, 10_000_000_000,
            "never repeats the last second"
        );
    }

    /// A console that cannot be reached stops the pairing and says why, rather than waiting for
    /// an approval that can never arrive. Which reason depends on the host: a port nothing
    /// listens on, or no `curl` to ask through, and both are the console being out of reach.
    #[test]
    fn an_unreachable_console_says_so() {
        let scratch = tormoni_test_support::ScratchDir::created("claim-unreachable");
        crate::device::create(scratch.path()).expect("a key");
        let why = refusal(claim("http://127.0.0.1:1", scratch.path(), 0).outcome);
        assert!(
            why.contains("could not be reached") || why.contains("curl is not installed"),
            "{why}"
        );
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

    /// The address is what the block shows, and what the lane must carry: the console's own
    /// `account_id` is a provider id that names nobody, and an answer without an address is one
    /// this cannot draw a person from.
    #[test]
    fn the_account_lane_answers_with_the_address_it_is_named_by() {
        // The shape the console actually serves, `account_id` and all.
        let read = identity(
            "{\"account_id\":\"user_01M0T\",\"display_name\":\"Kendrick Lawton\",\"email\":\"k@example.com\"}",
        )
        .expect("an identity");
        assert_eq!(read.email, "k@example.com");
        assert_eq!(read.display_name.as_deref(), Some("Kendrick Lawton"));

        let unnamed = identity("{\"account_id\":\"user_01M0T\",\"email\":\"k@example.com\"}")
            .expect("an identity");
        assert_eq!(unnamed.display_name, None);

        // An id alone is not an identity: nothing on the screen could be drawn from it.
        let why = identity("{\"account_id\":\"user_01M0T\"}").expect_err("no address");
        assert_eq!(why, "the account names no address");
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
    /// the console's answer may carry, and anything else in it is refused before curl sees it.
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
        let (cmd, config) =
            request("http://localhost:3000", ACCOUNT_LANE, "GET", &token).expect("a request");
        assert_eq!(cmd.get_program(), "curl");
        assert!(!argv(&cmd).contains("tor_secret"), "{}", argv(&cmd));
        assert!(argv(&cmd).contains("--config -"), "{}", argv(&cmd));
        assert!(
            argv(&cmd).ends_with("http://localhost:3000/v1/account"),
            "{}",
            argv(&cmd)
        );
        assert_eq!(config, "header = \"Authorization: Bearer tor_secret\"\n");
    }

    /// A curl with no `--data` sends a GET whatever the method is meant to be, so the sign-out
    /// carries `--request DELETE` or it reaches the lane as a read the lane refuses.
    #[test]
    fn the_sign_out_asks_the_device_lane_to_delete_and_names_no_device() {
        let token = Token::from("tor_secret".to_string());
        let (cmd, config) =
            request("http://localhost:3000", SIGN_OUT_LANE, "DELETE", &token).expect("a request");
        assert!(argv(&cmd).contains("--request DELETE"), "{}", argv(&cmd));
        assert!(
            argv(&cmd).ends_with("http://localhost:3000/v1/device"),
            "{}",
            argv(&cmd)
        );
        assert!(!argv(&cmd).contains("tor_secret"), "{}", argv(&cmd));
        assert_eq!(config, "header = \"Authorization: Bearer tor_secret\"\n");
    }

    /// What curl wrote, as one line for an assertion to read.
    fn argv(cmd: &Command) -> String {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The sign-out's two answers that mean the token is finished with, and the ones that do
    /// not: a console that refuses the token has already done what was asked, and a console
    /// that says something else is carried through in the operator's own words.
    #[test]
    fn a_token_the_console_no_longer_honours_is_a_sign_out_that_worked() {
        handed_back("\n204").expect("signed out");
        handed_back("{\"error\":\"a live tor_ API token is required\"}\n401")
            .expect("already gone");

        let stranger =
            handed_back("{\"detail\":\"this token was not minted by a paired device\"}\n404")
                .expect_err("not a device");
        assert_eq!(
            stranger,
            "it answered 404: this token was not minted by a paired device"
        );
        let down = handed_back("{\"error\":\"database unavailable\"}\n503").expect_err("down");
        assert_eq!(down, "it answered 503: database unavailable");
    }

    /// A console that cannot be reached is a row left listed, and the window is told in curl's
    /// own words rather than in a status this could not have known.
    ///
    /// That it is only ever a row, never a credential left on this disk, is the SIGNATURE and
    /// not this test: [`retire`] takes a token and no directory, so there is no file it could
    /// keep. The window wipes before it calls this, and
    /// `signing_out_empties_the_key_directory_before_it_asks_the_console_anything` is what
    /// holds it to that order.
    #[test]
    fn a_console_that_never_answers_is_a_sign_out_the_window_can_report() {
        // Nothing answers on the discard port, so the DELETE fails before it is written.
        let why = retire("http://127.0.0.1:9", "tor_secret".to_string()).expect_err("no console");
        assert!(
            why.contains("127.0.0.1") || why.contains("onnect"),
            "a refusal says what could not be reached: {why}"
        );
    }

    /// A token is never printed: the state it sits in is `Debug`, and a log line of that state
    /// would otherwise carry the credential.
    #[test]
    fn a_token_prints_as_nothing() {
        let shown = format!(
            "{:?}",
            Claimed::Token(Token::from("tor_secret".to_string()))
        );
        assert!(!shown.contains("secret"), "{shown}");
    }

    /// The console's answer becomes the identity, with or without a display name; a refusal
    /// and a stray status each become a line that says what to do, and an error body's own
    /// words are carried along.
    #[test]
    fn the_answer_becomes_an_identity_or_a_reason() {
        let named = answer("{\"email\":\"k@example.com\",\"display_name\":\"Kendrick L\"}\n200")
            .expect("signed in");
        assert_eq!(named.email, "k@example.com");
        assert_eq!(named.display_name.as_deref(), Some("Kendrick L"));
        let unnamed =
            answer("{\"email\":\"k@example.com\",\"display_name\":null}\n200").expect("signed in");
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
            "{\"account_id\":\"user_01M0T\",\"email\":\"k@example.com\",\"display_name\":null}",
        );
        let identity =
            begin(&console.origin(), &Token::from("tor_test".to_string())).expect("signed in");
        assert_eq!(identity.email, "k@example.com");
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
