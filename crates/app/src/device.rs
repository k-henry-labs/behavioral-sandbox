//! The device key this window pairs with, and the file it lives in.
//!
//! - **A key is what the console knows this machine by.** Pairing sends the public half in the
//!   `authorized_keys` line every SSH tool prints, and the app proves it holds the private half by
//!   signing [`claim_message`]. Nothing secret crosses the browser.
//! - **The fingerprint is the one a person compares.** [`Public::fingerprint`] is the string
//!   `ssh-keygen -lf` prints, over the whole decoded blob and not the bare key bytes, because that
//!   fingerprint is inside the signed message: the wrong one fails the signature with no clue why.
//! - **One key, one claim.** The console hands a key its token once, so a sign-in makes a new key
//!   rather than reusing the one a previous sign-in spent.
//! - **The private half is a file at `0600`, not a keychain.** This build has no credential store,
//!   and calling a file one would be a claim the tree cannot back.

use std::io::Read;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// The only key type the console accepts, and the name inside the blob.
const KEY_TYPE: &str = "ssh-ed25519";

/// The message a claim signs, which `contract/wire-contract.json` calls `device_claim.template`.
const TEMPLATE: &str = "tormoni-device-claim:v1";

/// Where the key and the token sit under the data directory.
const UNDER_DATA: &str = "tormoni/device";

/// The private half: 32 seed bytes, the file mode a key wants.
const KEY_FILE: &str = "device.key";

/// The public half, as `ssh-keygen -lf` will read it, so a person can check the fingerprint the
/// window shows without trusting the window.
const PUB_FILE: &str = "device.pub";

/// The token a claim answered with.
const TOKEN_FILE: &str = "token";

/// The public half of a device key: the line pairing sends, and the fingerprint a person reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Public {
    line: String,
    fingerprint: String,
}

impl Public {
    /// The `authorized_keys` line: `ssh-ed25519 <base64 of the wire blob>`.
    pub(crate) fn line(&self) -> &str {
        &self.line
    }

    /// The `SHA256:…` form `ssh-keygen -lf` prints.
    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The public half of `signing`, encoded as SSH encodes one.
    fn of(signing: &SigningKey) -> Self {
        let blob = blob(signing.verifying_key().as_bytes());
        Self {
            line: format!("{KEY_TYPE} {}", base64::encode(&blob)),
            fingerprint: fingerprint(&blob),
        }
    }

    /// The public half read back out of an `authorized_keys` line, refusing anything else.
    pub(crate) fn parse(line: &str) -> Result<Self, String> {
        let line = line.trim();
        let encoded = line
            .strip_prefix(KEY_TYPE)
            .map(str::trim_start)
            .and_then(|rest| rest.split_whitespace().next())
            .ok_or_else(|| format!("not an {KEY_TYPE} line"))?;
        let blob = base64::decode(encoded)?;
        if blob != blob_of(&blob)? {
            return Err("the blob does not carry an ed25519 key of its own name".to_string());
        }
        Ok(Self {
            line: format!("{KEY_TYPE} {encoded}"),
            fingerprint: fingerprint(&blob),
        })
    }
}

/// A device key: the private half, and the public half the console knows it by.
pub(crate) struct Key {
    signing: SigningKey,
    public: Public,
}

impl Key {
    pub(crate) fn public(&self) -> &Public {
        &self.public
    }

    /// The base64 signature over `message`, which is what a claim carries.
    pub(crate) fn sign(&self, message: &str) -> String {
        base64::encode(&self.signing.sign(message.as_bytes()).to_bytes())
    }
}

/// The message a claim at `issued_at` signs, with both holes filled.
pub(crate) fn claim_message(fingerprint: &str, issued_at: i64) -> String {
    format!("{TEMPLATE}:{fingerprint}:{issued_at}")
}

/// The SSH wire encoding of an ed25519 public key: a length-prefixed name, then the 32 bytes.
fn blob(key: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(51);
    blob.extend_from_slice(&u32::try_from(KEY_TYPE.len()).unwrap_or(0).to_be_bytes());
    blob.extend_from_slice(KEY_TYPE.as_bytes());
    blob.extend_from_slice(&32u32.to_be_bytes());
    blob.extend_from_slice(key);
    blob
}

/// The same blob rebuilt from what a parsed one claims to hold, so a line that is the right
/// length but the wrong shape is refused rather than fingerprinted.
fn blob_of(raw: &[u8]) -> Result<Vec<u8>, String> {
    let name_len = raw
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| "the blob is too short for a name".to_string())?;
    let name_end = 4usize
        .checked_add(usize::try_from(name_len).unwrap_or(usize::MAX))
        .ok_or_else(|| "the blob names an impossible length".to_string())?;
    if raw.get(4..name_end) != Some(KEY_TYPE.as_bytes()) {
        return Err(format!("the blob does not name {KEY_TYPE}"));
    }
    let key: [u8; 32] = raw
        .get(name_end + 4..)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or_else(|| "the blob does not hold 32 key bytes".to_string())?;
    Ok(blob(&key))
}

/// The OpenSSH fingerprint of a blob: `SHA256:` and the unpadded base64 of its digest.
fn fingerprint(blob: &[u8]) -> String {
    format!("SHA256:{}", base64::encode_unpadded(&Sha256::digest(blob)))
}

/// Where the key, its public half and the token live: beside the runs directory, under the same
/// data root, since a key is not a run record.
pub(crate) fn dir() -> Result<PathBuf, String> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("no HOME and no XDG_DATA_HOME, so there is nowhere to keep a device key")?;
    Ok(data.join(UNDER_DATA))
}

/// Makes a key for this sign-in and writes both halves, replacing whatever was there.
///
/// A new key each time, because the console hands a key its token once: the one a previous
/// sign-in spent could only ever be told it already collected.
pub(crate) fn create(dir: &Path) -> Result<Key, String> {
    let mut seed = [0u8; 32];
    // `/dev/urandom` on both platforms this builds for, rather than a crate that would ask the
    // same kernel through a per-platform branch.
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut seed))
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    let signing = SigningKey::from_bytes(&seed);
    seed.zeroize();
    let public = Public::of(&signing);

    make_dir(dir)?;
    write(&dir.join(KEY_FILE), signing.to_bytes().as_slice(), 0o600)?;
    write(&dir.join(PUB_FILE), public.line.as_bytes(), 0o644)?;
    Ok(Key { signing, public })
}

/// The key a pairing already made, so a claim can be signed without holding it on a screen.
pub(crate) fn load(dir: &Path) -> Result<Key, String> {
    let path = dir.join(KEY_FILE);
    let mut bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("{} does not hold 32 key bytes", path.display()))?;
    bytes.zeroize();
    let signing = SigningKey::from_bytes(&seed);
    let public = Public::of(&signing);
    // The public half is a file a person can run `ssh-keygen -lf` on, so it is worth knowing it
    // still describes the private half beside it.
    let beside = std::fs::read_to_string(dir.join(PUB_FILE))
        .map_err(|e| format!("read {}: {e}", dir.join(PUB_FILE).display()))?;
    if Public::parse(&beside)? != public {
        return Err(format!(
            "{} does not describe the key beside it",
            dir.join(PUB_FILE).display()
        ));
    }
    Ok(Key { signing, public })
}

/// Keeps `token` at `0600`, and says where, since this build has no credential store to put one
/// in and a file is what it is.
pub(crate) fn save_token(dir: &Path, token: &str) -> Result<(), String> {
    make_dir(dir)?;
    write(&dir.join(TOKEN_FILE), token.as_bytes(), 0o600)
}

/// Destroys everything this device was signed in with: the token and the key that claimed it.
pub(crate) fn forget(dir: &Path) -> Result<(), String> {
    for name in [TOKEN_FILE, KEY_FILE, PUB_FILE] {
        let path = dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("remove {}: {e}", path.display())),
        }
    }
    Ok(())
}

/// Creates `dir` at `0700`: what is under it is this person's key.
fn make_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod 0700 {}: {e}", dir.display()))
}

/// Writes `bytes` to `path` at `mode`, replacing what was there.
fn write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {mode:o} {}: {e}", path.display()))
}

/// Base64, the one encoding the SSH line, the fingerprint and the signature all need. Written
/// here rather than taken as a dependency: it is two tables and two loops, and the pinned
/// `ssh-keygen` vector is what checks it.
mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// The padded form, which an `authorized_keys` line and a signature carry.
    pub(super) fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let (b0, b1, b2) = (
                u32::from(chunk[0]),
                chunk.get(1).map_or(0, |b| u32::from(*b)),
                chunk.get(2).map_or(0, |b| u32::from(*b)),
            );
            let word = (b0 << 16) | (b1 << 8) | b2;
            for i in 0..4 {
                if i <= chunk.len() {
                    let index = (word >> (18 - 6 * i)) & 0x3f;
                    out.push(char::from(ALPHABET[index as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// The unpadded form, which is how a fingerprint is spelled.
    pub(super) fn encode_unpadded(bytes: &[u8]) -> String {
        let mut out = encode(bytes);
        out.truncate(out.trim_end_matches('=').len());
        out
    }

    /// The bytes behind an encoding, padded or not; anything outside the alphabet is refused.
    pub(super) fn decode(text: &str) -> Result<Vec<u8>, String> {
        let mut bits = 0u32;
        let mut held = 0u32;
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        for c in text.trim_end_matches('=').bytes() {
            let value = ALPHABET
                .iter()
                .position(|a| *a == c)
                .ok_or_else(|| format!("{:?} is not base64", char::from(c)))?;
            bits = (bits << 6) | value as u32;
            held += 6;
            if held >= 8 {
                held -= 8;
                out.push(u8::try_from((bits >> held) & 0xff).unwrap_or(0));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tormoni_test_support::ScratchDir;

    /// The line `ssh-keygen -lf` was run against on this host, 2026-09-09, and the fingerprint it
    /// printed. The whole decoded blob is hashed, not the 32 key bytes: that is the difference
    /// between a claim the console verifies and one it refuses with nothing to point at.
    const VECTOR_LINE: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFa10MA76wL6dM6M0E8ywmtRRgZmS7piItAo510hY6bb";
    const VECTOR_FINGERPRINT: &str = "SHA256:oukpkX43xeppqEo85rexHqFcw3I3pR2ZIdRfIr9KOBo";

    /// The fingerprint is the one OpenSSH prints, and hashing the bare key bytes is a different
    /// string: the wrong one rides inside the signed message and fails the signature check.
    #[test]
    fn the_fingerprint_is_the_one_ssh_keygen_prints() {
        let public = Public::parse(VECTOR_LINE).expect("the vector parses");
        assert_eq!(public.fingerprint(), VECTOR_FINGERPRINT);
        assert_eq!(public.line(), VECTOR_LINE);

        let blob = base64::decode(
            VECTOR_LINE
                .split_whitespace()
                .nth(1)
                .expect("the encoded half"),
        )
        .expect("base64");
        assert_eq!(blob.len(), 51, "name, lengths and 32 key bytes");
        let bare = format!(
            "SHA256:{}",
            base64::encode_unpadded(&Sha256::digest(&blob[19..]))
        );
        assert_ne!(
            bare, VECTOR_FINGERPRINT,
            "hashing the bare key bytes must not be mistaken for the fingerprint"
        );
    }

    /// A key this build makes writes a line this build reads back to the same fingerprint, so the
    /// string shown beside Sign In is the string the console derives from what it was sent.
    #[test]
    fn a_line_round_trips_to_the_same_fingerprint() {
        let scratch = ScratchDir::created("device-round-trip");
        let key = create(scratch.path()).expect("a key");
        let parsed = Public::parse(key.public().line()).expect("our own line parses");
        assert_eq!(&parsed, key.public());
        assert_eq!(parsed.fingerprint(), key.public().fingerprint());
        assert!(parsed.fingerprint().starts_with("SHA256:"));

        let reloaded = load(scratch.path()).expect("the key reads back");
        assert_eq!(
            reloaded.public(),
            key.public(),
            "the same key, not a new one"
        );
    }

    /// The signed message is the console's template with both holes filled, character for
    /// character: the console rebuilds this string and verifies against it.
    #[test]
    fn the_signed_message_is_the_template_with_both_holes_filled() {
        assert_eq!(
            claim_message(VECTOR_FINGERPRINT, 1_789_000_000),
            "tormoni-device-claim:v1:SHA256:oukpkX43xeppqEo85rexHqFcw3I3pR2ZIdRfIr9KOBo:1789000000"
        );
    }

    /// RFC 8032's first test vector. The console verifies with `ring` and this signs with
    /// `ed25519-dalek`, so what holds the two together is the published answer, not shared code.
    #[test]
    fn an_rfc_8032_vector_signs_to_the_published_bytes() {
        let seed: [u8; 32] =
            hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
                .try_into()
                .expect("32 bytes");
        let key = Key {
            signing: SigningKey::from_bytes(&seed),
            public: Public::of(&SigningKey::from_bytes(&seed)),
        };
        assert_eq!(
            key.signing.verifying_key().as_bytes().as_slice(),
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
            "the public half RFC 8032 names"
        );
        assert_eq!(
            base64::decode(&key.sign("")).expect("our own base64"),
            hex(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            ),
            "the signature RFC 8032 publishes for the empty message"
        );
    }

    /// Base64 round-trips, pads as the SSH line does, and refuses a character outside the
    /// alphabet rather than inventing a byte for it.
    #[test]
    fn base64_round_trips_and_refuses_what_is_not_base64() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let encoded = base64::encode(bytes);
            assert_eq!(encoded.len() % 4, 0, "padded to a multiple of four");
            assert_eq!(base64::decode(&encoded).expect("round trip"), bytes);
        }
        assert_eq!(base64::encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64::encode(b"fo"), "Zm8=");
        assert_eq!(base64::encode_unpadded(b"fo"), "Zm8");
        let why = base64::decode("not base64!").expect_err("refused");
        assert!(why.contains("not base64"), "{why}");
    }

    /// A line that is not this key type, or whose blob does not hold what it says, is refused
    /// rather than fingerprinted into something the console would never match.
    #[test]
    fn a_line_that_is_not_an_ed25519_key_is_refused() {
        for line in [
            "ssh-rsa AAAAB3NzaC1yc2E",
            "ssh-ed25519",
            "ssh-ed25519 not-base64!",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5",
        ] {
            assert!(Public::parse(line).is_err(), "{line} was accepted");
        }
    }

    /// The private half is readable by nobody else, and so is the directory holding it. A key
    /// that any other account could read is a device somebody else can be.
    #[test]
    fn the_private_half_and_its_directory_are_this_persons_alone() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = ScratchDir::created("device-modes");
        let dir = scratch.path().join("device");
        create(&dir).expect("a key");
        let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700, "the directory");
        assert_eq!(mode(&dir.join(KEY_FILE)), 0o600, "the private half");

        save_token(&dir, "tor_example").expect("saved");
        assert_eq!(mode(&dir.join(TOKEN_FILE)), 0o600, "the token");

        forget(&dir).expect("forgotten");
        for name in [KEY_FILE, PUB_FILE, TOKEN_FILE] {
            assert!(!dir.join(name).exists(), "{name} survived a sign-out");
        }
        forget(&dir).expect("forgetting twice is not an error");
    }

    /// The bytes behind a hex string, for the published vectors above.
    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex")
            })
            .collect()
    }
}
