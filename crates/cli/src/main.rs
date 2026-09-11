//! The `boxdesk` CLI, and the hidden helper subcommand that becomes a virtual machine.
//!
//! `boxdesk run` boots a sandbox, runs one command, and exits with its status ([`run`]); `boxdesk shell`
//! opens an interactive session on a guest pty ([`shell`]). Beside them
//! sits `__vmm`, which is not a verb anyone types: it is how a VM comes into existence, and
//! [`vmm`] explains why that has to be a whole process. The rest of the verbs arrive with
//! `scratch/ROADMAP.md` phase 3.
//!
//! stdout stays reserved for the guest's own output, so the pipe contract holds: what the command
//! in the sandbox writes is what `boxdesk run` writes.
#![forbid(unsafe_code)]

mod agent;
mod frames;
mod image;
mod input;
mod json;
mod lifecycle;
mod posture;
mod pty;
mod run;
mod serve;
mod shell;
mod snapshot;
mod up;
mod vmm;
mod volume;
mod window;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Exit code for an operational failure, as opposed to a guest command's own exit code:
/// conventional "2", the same convention (and name) as the guest agent's.
const EXIT_OPERATIONAL: u8 = 2;

/// Refuses a `--name` the filesystem could not carry, quoting `boxdesk-supervisor`'s rule.
///
/// Checked where the flag was typed, or a refusal further in names the socket or the run id.
fn check_name(name: &str) -> Result<(), String> {
    if boxdesk_supervisor::socket::valid_name(name) {
        return Ok(());
    }
    Err(format!(
        "{name:?} is not a usable --name: {}, since the name becomes a filename",
        boxdesk_supervisor::socket::name_rule()
    ))
}

#[derive(Parser)]
#[command(
    name = "boxdesk",
    version,
    about = "Run untrusted code in a hardware-isolated sandbox.",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run one command in a fresh sandbox and exit with its status.
    Run(run::RunArgs),
    /// Open an interactive shell (or any command) on a pty in a fresh sandbox.
    Shell(shell::ShellArgs),
    /// Start a sandbox that outlives this command, reachable afterwards by name.
    Up(up::UpArgs),
    /// List the sandboxes running on this machine.
    Ls(lifecycle::LsArgs),
    /// Run a command in a sandbox that is already up.
    Exec(lifecycle::ExecArgs),
    /// Stop a running sandbox.
    Stop(lifecycle::StopArgs),
    /// Show one run's record: what it could touch, what it printed, what it wrote.
    Show(lifecycle::ShowArgs),
    /// Export one run's record, captured output and results as a tar file.
    Export(lifecycle::ExportArgs),
    /// Remove one run's record and everything it captured.
    Rm(lifecycle::RmArgs),
    /// Sandboxes worth making again, kept by name: `new`, `ls`, `show`, `rm`.
    Snapshot(snapshot::SnapshotArgs),
    /// Fetch an OCI image and flatten it into a guest root this machine can boot.
    Pull(image::PullArgs),
    /// The images pulled onto this machine: `ls`, `rm`.
    Image(image::ImageArgs),
    /// Where images come from, and who this machine is when it asks: `add`, `ls`, `rm`.
    Registry(image::RegistryArgs),
    /// Directories with lives of their own, mounted into sandboxes by name: `new`, `ls`, `show`, `rm`.
    Volume(volume::VolumeArgs),
    /// Run sandboxes for a caller over HTTP: one box, one token, one tenant.
    Serve(serve::ServeArgs),
    /// Send runs to the console and read back what it holds.
    /// Become a virtual machine. Not a verb: the supervisor re-executes this binary with it.
    ///
    /// Hidden rather than removed from the parser, so a boot that fails can be reproduced by hand
    /// with the exact arguments the supervisor used.
    #[command(name = vmm::HELPER_SUBCOMMAND, hide = true)]
    Vmm(vmm::VmmArgs),
    /// Read a running sandbox's display through the control socket. A development verb: what
    /// `cargo xtask bench-frames` runs to time the process boundary, and the end-to-end test's
    /// proof that a second process sees the frames.
    #[command(name = "__frames", hide = true)]
    Frames(frames::FramesArgs),
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Run(args) => run::run(&args),
        Cmd::Shell(args) => shell::run(&args),
        Cmd::Up(args) => up::run(&args),
        Cmd::Ls(args) => lifecycle::ls(&args),
        Cmd::Exec(args) => lifecycle::exec(&args),
        Cmd::Stop(args) => lifecycle::stop(&args),
        Cmd::Show(args) => lifecycle::show(&args),
        Cmd::Export(args) => lifecycle::export(&args),
        Cmd::Rm(args) => lifecycle::rm(&args),
        Cmd::Snapshot(args) => snapshot::run(&args),
        Cmd::Pull(args) => image::run_pull(&args),
        Cmd::Image(args) => image::run_image(&args),
        Cmd::Registry(args) => image::run_registry(&args),
        Cmd::Volume(args) => volume::run(&args),
        Cmd::Serve(args) => serve::run(&args),
        Cmd::Vmm(args) => vmm::run(&args),
        Cmd::Frames(args) => frames::run(&args),
    }
}
