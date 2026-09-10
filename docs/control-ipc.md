# Control socket and host/guest IPC

Tormoni uses a two-tier IPC design for host management and host↔guest
communication: a host control socket for local process discovery and display leasing, and a
virtio-vsock wire protocol for in-guest command execution.

## Host control socket

There is no background daemon. A running sandbox is a helper process (`tormoni __vmm`) listening on a
Unix domain socket under the user's runtime directory.

### Socket resolution and discovery

Sockets live in `$XDG_RUNTIME_DIR/tormoni/<name>.sock` (falling back to `$TMPDIR/tormoni/<name>.sock` or
`/tmp/tormoni/<name>.sock`). The directory is created `0700` and its ownership and mode are checked at
runtime, before a socket in it is trusted, because the fallbacks are shared temporary directories.

The socket directory acts as the VM registry:
- **Discovery**: `tormoni ls` scans the socket directory for files ending in `.sock`.
- **Liveness probe**: Rather than relying on file existence, `socket::is_live` attempts a
  non-blocking `UnixStream::connect`. A socket file whose process has died is cleaned up via
  `socket::clear_if_stale`.
- **Sidecar paths**: The agent socket sits at `<name>.agent` and the detached log file sits at
  `<name>.log`.

### Control protocol

A request is one word on one line, and the answer begins `ok` or `err <why>`; a caller reads a reply
to 4096 bytes at most. A word the VM does not know is answered with the words it does speak rather
than a closed connection (`an_unknown_request_is_answered_with_what_this_vm_speaks`).

- `info`: `ok`, then the machine's shape and posture as `key value` lines (`proto`, `pid`, `vcpus`,
  `mem_mib`, `net`, `rootfs`, `channel`), which is what `tormoni ls` prints a row from.
- `stop`: `ok` first, and the process exits after, so a caller learns the request was accepted
  rather than inferring it from a closed connection.
- `display`: leases the scanout. The answer carries the sealed memfd holding the frame slots and
  their layout over `SCM_RIGHTS`, and the connection then streams one record per present until the
  caller closes it. Refused by a VM with no display.
- `input`: after `ok`, the connection carries `kbd|ptr TYPE CODE VALUE` lines, one event each,
  until the caller closes it, and whatever those lines left pressed is released then. Refused by a
  VM with no display, which has no devices.

## Host↔guest wire framing (`tormoni-channel`)

Command execution inside a guest goes through `tormoni-channel`, a length-prefixed wire protocol
operating over AF_VSOCK (port 1024) or a Unix socket fallback.

### Handshake and framing

Every session begins with a 4-byte magic header (`AGCH`) and a 2-byte protocol version (`u16` = 3):
1. Both host (`ClientConnection`) and guest (`ServerConnection`) exchange magic and version headers.
2. Version mismatches reject immediately. 3. Subsequent messages use length-prefixed framing:
`tag(u8) · len(u32-le) · payload`.

`len` is checked against `MAX_PAYLOAD` (1 MiB) before anything is allocated, so a length a guest
chose cannot size a host allocation.

### Message tags

The wire protocol defines discrete frame discriminants:

| Tag | Name | Direction | Payload |
|---|---|---|---|
| 1 | `Exec` | Host → Guest | Command string, arguments, working directory, and environment key-value pairs. |
| 2 | `Stdout` | Guest → Host | Binary output stream from the command's stdout. |
| 3 | `Stderr` | Guest → Host | Binary output stream from the command's stderr. |
| 4 | `Exit` | Guest → Host | Command exit code (`i32`). |
| 5 | `Error` | Guest → Host | Agent error message string (sanitized). |
| 6 | `PutFile` | Host → Guest | Injected file path and binary content. |
| 7 | `File` | Guest → Host | Extracted file content from guest `/results`. |
| 8 | `TimedOut` | Guest → Host | Indicates command execution exceeded the configured deadline. |
| 9 | `ExecPty` | Host → Guest | Interactive shell request with PTY window dimensions (`cols`, `rows`). |
| 10 | `Stdin` | Host → Guest | Binary input stream for stdin or PTY interactive sessions. |
| 11 | `Resize` | Host → Guest | Updated PTY window dimensions (`cols`, `rows`). |

### Security and sanitization

- **Secret wiping**: Sensitive environment variables and payload buffers implement `zeroize` to wipe
  secret memory upon drop.
- **Error sanitization**: Guest error messages are capped at 4 KiB (`ERROR_MSG_CAP`) and escaped for
  ASCII control characters and Unicode bidirectional control code points (`Bidi_Control`), which are
  what a terminal would otherwise act on and what Trojan Source relies on.

## In-guest agent (`tormoni-guest-agent`)

The guest agent is a statically linked Rust binary (`guest-agent`, compiled against
`x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl`) baked into the guest image at
`/usr/local/bin/guest-agent`.

- **Role**: Serves command execution requests (`Exec`/`ExecPty`), manages process lifecycles,
  attaches pseudo-terminals (PTYs), and handles file reads/writes in `/results`.
- **Trust boundary**: The agent runs inside the guest and is **not** part of the host isolation
  boundary. What contains a compromised agent is the CPU, through KVM or Hypervisor.framework, not
  anything the agent does.
