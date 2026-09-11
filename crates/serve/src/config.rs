//! What the server was told, and what this host can actually do.
//!
//! - **Loopback unless somebody says otherwise.** A sandbox host that comes up on `0.0.0.0` the
//!   first time it is tried is a bad default with a long tail. The wider bind is explicit.
//! - **A token is a credential.** It is read from a file this user owns at `0600`, or from the
//!   environment for a container that has no file. It is never an argument, because a process list
//!   is public, and it is never printed.
//! - **The environment overrides the file, and the flag overrides both**, so a container runs this
//!   with no file at all.
//! - **No cloud anywhere.** Nothing here reaches a host of ours. A self-hoster generates a token,
//!   writes it down, points a client at the box, and is done.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

/// The port `tormoni serve` listens on when nothing names one.
pub const DEFAULT_PORT: u16 = 8420;
/// The bind address, the token and the data directory, for a container with no config file.
pub const BIND_ENV: &str = "TORMONI_SERVE_BIND";
pub const TOKEN_ENV: &str = "TORMONI_SERVE_TOKEN";
pub const DATA_ENV: &str = "TORMONI_SERVE_DATA";
/// How many sandboxes may run at once when nothing names a number.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// Everything the server needs to start.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where to listen. Loopback unless told otherwise.
    pub bind: SocketAddr,
    /// The one credential this box accepts.
    pub token: String,
    /// Where the ledger and anything else this server keeps lives.
    pub data: PathBuf,
    /// The most sandboxes that may run at once.
    pub concurrency: usize,
}

/// Why a server could not start. Each names the thing to fix rather than the thing that failed.
#[derive(Debug)]
pub enum Refusal {
    /// No token was found anywhere, so every request would be refused.
    NoToken(String),
    /// The token file is readable by somebody other than its owner.
    TokenReadable(PathBuf),
    /// This host cannot run a sandbox at all.
    NoHypervisor(String),
    /// Something on this machine would not answer.
    Local(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoToken(where_to_put_it) => write!(f, "{where_to_put_it}"),
            Self::TokenReadable(path) => write!(
                f,
                "{} can be read by somebody other than its owner; `chmod 0600 {}` first",
                path.display(),
                path.display()
            ),
            Self::NoHypervisor(why) => write!(f, "{why}"),
            Self::Local(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// The address to listen on: the flag, else the environment, else loopback on [`DEFAULT_PORT`].
///
/// A bare port is loopback on that port, so `--bind 9000` cannot accidentally mean the world.
pub fn bind(flag: Option<&str>, env: Option<String>) -> Result<SocketAddr, Refusal> {
    let Some(named) = flag.map(str::to_string).or(env) else {
        return Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            DEFAULT_PORT,
        ));
    };
    let named = named.trim();
    if let Ok(port) = named.parse::<u16>() {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    named.parse().map_err(|_| {
        Refusal::Local(format!(
            "{named:?} is not an address to listen on: give `PORT`, `127.0.0.1:PORT` or `0.0.0.0:PORT`"
        ))
    })
}

/// Whether this bind reaches past the machine, for the line an operator should see before it is
/// too late to have meant something else.
#[must_use]
pub fn is_public(bind: &SocketAddr) -> bool {
    !bind.ip().is_loopback()
}

/// The token: the environment first, for a container, then the file.
///
/// Nothing here mints one. A self-hoster writes a secret of their own choosing into the file;
/// this only reads it, and refuses to read one anybody else on the machine could.
pub fn token(env: Option<String>, file: &Path) -> Result<String, Refusal> {
    if let Some(from_env) = env
        && !from_env.trim().is_empty()
    {
        return Ok(from_env.trim().to_string());
    }
    match std::fs::read_to_string(file) {
        Ok(held) if !held.trim().is_empty() => {
            refuse_if_readable(file)?;
            Ok(held.trim().to_string())
        }
        _ => Err(Refusal::NoToken(format!(
            "no token, so every request would be refused. Write one to {} at mode 0600, or set ${TOKEN_ENV}",
            file.display()
        ))),
    }
}

/// Refuses a token file that anybody but its owner can read.
///
/// A credential at `0644` on a shared box is a credential every account on it holds. Checked
/// rather than fixed: silently re-`chmod`ing somebody's file hides that it was ever wrong.
fn refuse_if_readable(path: &Path) -> Result<(), Refusal> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|e| Refusal::Local(format!("stat {}: {e}", path.display())))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(Refusal::TokenReadable(path.to_path_buf()));
        }
    }
    Ok(())
}

/// Whether a presented bearer is the one this box accepts.
///
/// Compared in constant time over the whole of both: a comparison that stops at the first wrong
/// byte tells an attacker how much of a guess was right.
#[must_use]
pub fn accepts(expected: &str, presented: &str) -> bool {
    if expected.len() != presented.len() {
        return false;
    }
    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Whether this host can run a sandbox at all, or why it cannot.
///
/// Asked as a capability rather than read off the platform's name, and asked once at startup:
/// a caller who posts a job to a box with no hypervisor should be told by the box refusing to
/// start, not by a VM that never boots.
#[must_use]
pub fn hypervisor_unusable() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
        {
            Ok(_) => None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(
                "/dev/kvm is absent, so this machine cannot run a sandbox. On a cloud VM this \
                 usually means nested virtualisation is off: it is Linux KVM only, and is not \
                 offered on GCE E2 machine types, on Arm, or on AMD except N4D"
                    .to_string(),
            ),
            Err(e) => Some(format!(
                "/dev/kvm cannot be opened read-write ({e}). That is a host permission and no \
                 reinstall changes it: add this user to the `kvm` group and log in again"
            )),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let answers = std::process::Command::new("sysctl")
            .args(["-n", "kern.hv_support"])
            .output()
            .is_ok_and(|out| out.status.success() && out.stdout.starts_with(b"1"));
        (!answers).then(|| {
            "this machine reports no Hypervisor.framework support, so it cannot run a sandbox"
                .to_string()
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Some("this platform has no hypervisor this build knows how to ask for".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tormoni_test_support::ScratchDir;

    /// **Loopback unless somebody was explicit.** A bare port is loopback on that port, so the
    /// shortest thing to type is never the one that publishes a sandbox host to the network.
    #[test]
    fn a_bind_is_loopback_until_it_is_spelled_out() {
        let default = bind(None, None).expect("a default");
        assert!(default.ip().is_loopback(), "{default}");
        assert_eq!(default.port(), DEFAULT_PORT);
        assert!(!is_public(&default));

        let bare = bind(Some("9000"), None).expect("a port");
        assert_eq!(
            bare.to_string(),
            "127.0.0.1:9000",
            "a bare port is loopback"
        );
        assert!(!is_public(&bare));

        // Reaching past the machine takes saying so, and is reported as public so an operator is
        // told before it is too late to have meant something else.
        let wide = bind(Some("0.0.0.0:9000"), None).expect("an address");
        assert!(is_public(&wide), "{wide}");

        // The environment is the container's way in, and the flag outranks it.
        assert_eq!(
            bind(None, Some("0.0.0.0:7000".into())).expect("the environment"),
            "0.0.0.0:7000".parse::<SocketAddr>().expect("an address")
        );
        assert!(bind(Some("not-an-address"), None).is_err());
    }

    /// A token file anybody else can read is refused rather than quietly used, and rather than
    /// quietly fixed: a credential at 0644 on a shared box belongs to every account on it.
    #[test]
    fn a_token_readable_by_anybody_else_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = ScratchDir::created("serve-token");
        let path = scratch.path().join("token");
        std::fs::write(&path, "tor_secret\n").expect("written");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert_eq!(token(None, &path).expect("a token"), "tor_secret");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let why = token(None, &path)
            .expect_err("readable by others")
            .to_string();
        assert!(why.contains("0600"), "{why}");

        // The environment is for a container with no file, and outranks one.
        assert_eq!(
            token(Some("tor_from_env".into()), &path).expect("the environment"),
            "tor_from_env"
        );
    }

    /// With no token nothing could be served, so the server says where to put one rather than
    /// starting and refusing everything.
    #[test]
    fn no_token_anywhere_names_the_file_to_write() {
        let scratch = ScratchDir::created("serve-no-token");
        let why = token(None, &scratch.path().join("absent"))
            .expect_err("no token")
            .to_string();
        assert!(why.contains("absent"), "names the file: {why}");
        assert!(why.contains(TOKEN_ENV), "names the variable: {why}");
    }

    /// A bearer is compared over the whole of both, so the time taken says nothing about how much
    /// of a guess was right.
    #[test]
    fn a_bearer_is_compared_to_the_end() {
        assert!(accepts("tor_secret", "tor_secret"));
        assert!(!accepts("tor_secret", "tor_secres"));
        assert!(!accepts("tor_secret", "tor_secret "));
        assert!(!accepts("tor_secret", ""));
        assert!(!accepts("", "tor_secret"));
    }
}
