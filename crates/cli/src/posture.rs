//! The posture flags every verb that boots shares, and the helper parses back off its own argv.
//!
//! - **One spelling, both sides.** `run`, `shell` and `up` take these from the person, and `__vmm`
//!   takes the same words back off the argv the supervisor wrote. Two enums of one shape would be
//!   two lists a new posture has to reach, and the second is the one that gets missed.
//! - **The CLI owns the mirror.** clap's `ValueEnum` cannot derive on a type from another crate,
//!   so these mirror `boxdesk_supervisor`'s; the `into_*` methods below are the only crossings,
//!   and `the_record_and_the_config_spell_the_posture_alike` holds the words in step.
//! - **Each default is the closed one**, because libkrun's own defaults are not: it adds an
//!   implicit vsock whose TSI hijacking proxies the guest's sockets onto the host, so saying
//!   nothing has to mean no network here.

/// What the guest's network reaches.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum NetArg {
    /// No network beyond loopback. The default, because libkrun's implicit TSI vsock is not.
    #[default]
    None,
    /// libkrun's transparent socket impersonation: the guest reaches what the host can. Opt-in.
    Tsi,
}

impl NetArg {
    /// The supervisor's spelling of this posture.
    pub(crate) fn into_net(self) -> boxdesk_supervisor::Net {
        match self {
            Self::None => boxdesk_supervisor::Net::None,
            Self::Tsi => boxdesk_supervisor::Net::Tsi,
        }
    }

    /// The TSI flags libkrun's vsock device carries for this posture. `0` is the explicit device
    /// that replaces the implicit one, which is what makes [`None`](Self::None) mean it.
    pub(crate) fn tsi_flags(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Tsi => boxdesk_krun::KRUN_TSI_HIJACK_INET,
        }
    }
}

/// What the guest may do to the image tree it boots from, which every sandbox on this host
/// shares: a writable root is a guest editing what the next guest starts from.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum RootFsArg {
    /// The guest cannot write its root. The default: one image tree boots every sandbox.
    #[default]
    ReadOnly,
    /// The guest writes through to the shared image tree, and its edits outlive the VM.
    Writable,
}

impl RootFsArg {
    /// The supervisor's spelling of this posture.
    pub(crate) fn into_rootfs(self) -> boxdesk_supervisor::RootFs {
        match self {
            Self::ReadOnly => boxdesk_supervisor::RootFs::ReadOnly,
            Self::Writable => boxdesk_supervisor::RootFs::Writable,
        }
    }

    /// The virtiofs device flag this posture is. Enforced by the device, so the guest can neither
    /// undo nor see it: `/proc/mounts` still reports the root `rw`.
    pub(crate) fn into_access(self) -> boxdesk_krun::FsAccess {
        match self {
            Self::ReadOnly => boxdesk_krun::FsAccess::ReadOnly,
            Self::Writable => boxdesk_krun::FsAccess::ReadWrite,
        }
    }
}

/// The snapshot a verb was told to start from, read once so the verb can ask it for each posture
/// the flags left unsaid.
///
/// # Errors
///
/// The store cannot be opened, or there is no snapshot of that name.
pub(crate) fn asked_for(name: Option<&str>) -> Result<Option<boxdesk_record::Snapshot>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let store = boxdesk_record::SnapshotStore::open().map_err(|e| e.to_string())?;
    store.read(name).map(Some).map_err(|e| {
        // A name nobody wrote is the common mistake, and `No such file or directory` names the
        // file rather than the thing: say what was not found, and where the list of them is.
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("no snapshot named {name:?} (`boxdesk snapshot ls` lists them)")
        } else {
            format!("--snapshot {name:?}: {e}")
        }
    })
}

/// Lays a snapshot's posture into `cfg` as the base the verb's own flags then speak over.
///
/// **The order is flag, then snapshot, then environment.** A flag is this command; a snapshot is
/// a thing you asked for by name; `$BOXDESK_VCPUS` and its kind are this machine's ambient
/// default. Being asked for by name should outrank the air, and typing a flag should outrank
/// both. Each caller applies its own flags after this, and each of those flags only speaks when
/// it was actually given, which is what leaves the snapshot standing where one was not.
///
/// **`sound` and `gpu` are unions, not overrides**, because their flags can only turn them on:
/// there is no `--no-sound` to mean "not this time", so a flag can add to what a snapshot said
/// and never take away. `--no-results` is the one that subtracts, and its caller applies it.
pub(crate) fn lay_snapshot(
    cfg: &mut boxdesk_supervisor::VmConfig,
    snapshot: &boxdesk_record::Snapshot,
) {
    let p = &snapshot.posture;
    // Both of these are `#[non_exhaustive]`, and both arms below fall closed. A snapshot written
    // by a later build could name a posture this one has never heard of; opening the network, or
    // the root, because a word was unfamiliar is the wrong way to be wrong.
    cfg.net = match p.network {
        boxdesk_record::Network::Tsi => boxdesk_supervisor::Net::Tsi,
        _ => boxdesk_supervisor::Net::None,
    };
    cfg.rootfs = match p.rootfs {
        boxdesk_record::Rootfs::Writable => boxdesk_supervisor::RootFs::Writable,
        _ => boxdesk_supervisor::RootFs::ReadOnly,
    };
    cfg.sound |= p.sound;
    cfg.gpu |= p.gpu;
    // Not the limits: those go through `limit` below, which is the one place their precedence is
    // written, so there is no second path that could disagree with it.
    // A snapshot's own mounts and shares come first, then the run's: a template says what every
    // sandbox from it gets, and a run adds this one's project on top rather than replacing it.
    for m in &p.mounts {
        cfg.mounts.push((m.guest.clone(), m.host.clone()));
    }
    for s in &p.shares {
        cfg.shares.push((s.tag.clone(), s.host.clone()));
    }
    // Never `p.env`: a posture keeps the NAMES of environment entries and not what they are set
    // to, so there is nothing here a guest could be given. `snapshot new` takes no `--env` for
    // exactly that reason, so this is a rule with nothing to drop rather than a silent loss.
}

/// The display a snapshot asks for, in the supervisor's spelling, so a verb can `.or()` it under
/// its own `--display`.
///
/// Not part of [`lay_snapshot`], because `apply_display` assigns the field rather than adding to
/// it: laying one in here and having that overwrite it a line later is the shape of bug this
/// whole file is arranged to avoid.
pub(crate) fn snapshot_display(
    snapshot: Option<&boxdesk_record::Snapshot>,
) -> Option<boxdesk_supervisor::Display> {
    let mode = snapshot?.posture.display?;
    let display = boxdesk_supervisor::Display::new(mode.width, mode.height);
    Some(match mode.refresh {
        Some(hz) => display.with_refresh(hz),
        None => display,
    })
}

/// A limit's value by the one precedence every verb that boots uses: the flag, then the snapshot,
/// then the environment variable, then nothing — which leaves whatever default the config held.
///
/// **Asked for by name outranks the air.** `$BOXDESK_VCPUS` is this machine's ambient setting;
/// naming a snapshot is a choice made about this sandbox, so it wins. Typing the flag wins over
/// both, because it is a choice made about this command.
///
/// # Errors
///
/// The variable is set to something that is not a usable limit, which is refused loudly rather
/// than ignored: a typo'd limit that silently falls back is a config that lies.
pub(crate) fn limit<T: std::str::FromStr>(
    flag: Option<T>,
    from_snapshot: Option<T>,
    var: &'static str,
) -> Result<Option<T>, String> {
    if flag.is_some() {
        return Ok(flag);
    }
    if from_snapshot.is_some() {
        return Ok(from_snapshot);
    }
    crate::run::resolve_limit(None, var)
}

/// The command a run should boot, which is the one it was given or else the one its snapshot
/// carries.
///
/// # Errors
///
/// Neither said anything, so there is nothing to run.
pub(crate) fn command_for(
    given: &[String],
    snapshot: Option<&boxdesk_record::Snapshot>,
) -> Result<Vec<String>, String> {
    if !given.is_empty() {
        return Ok(given.to_vec());
    }
    match snapshot {
        Some(s) if !s.command.is_empty() => Ok(s.command.clone()),
        Some(s) => Err(format!(
            "no command after `--`, and the snapshot {:?} carries none",
            s.name
        )),
        None => Err("no command after `--`".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot for a test, with a posture nothing about it is the default.
    fn snapshot() -> boxdesk_record::Snapshot {
        let mut posture = boxdesk_record::Posture::new(
            std::path::PathBuf::from("/srv/guest"),
            std::num::NonZeroU8::new(1).expect("non-zero"),
            std::num::NonZeroU32::new(512).expect("non-zero"),
        );
        posture.network = boxdesk_record::Network::Tsi;
        posture.rootfs = boxdesk_record::Rootfs::Writable;
        posture.sound = true;
        posture.vcpus = std::num::NonZeroU8::new(4).expect("4 is non-zero");
        posture.mem_mib = std::num::NonZeroU32::new(4096).expect("4096 is non-zero");
        boxdesk_record::Snapshot::new("devbox", posture, vec!["true".to_string()])
    }

    /// Every posture a snapshot holds is laid into the config, which is the whole of what
    /// starting from one means.
    #[test]
    fn a_snapshots_posture_is_what_a_sandbox_from_it_starts_with() {
        let mut cfg = boxdesk_supervisor::VmConfig::new(std::path::PathBuf::from("/r"), "true");
        lay_snapshot(&mut cfg, &snapshot());
        assert_eq!(cfg.net, boxdesk_supervisor::Net::Tsi);
        assert_eq!(cfg.rootfs, boxdesk_supervisor::RootFs::Writable);
        assert!(cfg.sound, "the snapshot asked for a sound card");
        assert!(!cfg.gpu, "and did not ask for the GPU");
    }

    /// **A posture this build has never heard of falls closed.** Both of the record's posture
    /// enums are `#[non_exhaustive]`, so a snapshot written by a later build can name a word this
    /// one does not know; opening the network or the root because of that would be the wrong way
    /// to be wrong. The check is that the closed variant is what the unknown arm lands on.
    #[test]
    fn an_unknown_posture_is_the_closed_one() {
        let mut snapshot = snapshot();
        // The two closed variants, asserted as the values the `_` arms produce: if a later build
        // renames or reorders them, this is where that shows up.
        snapshot.posture.network = boxdesk_record::Network::None;
        snapshot.posture.rootfs = boxdesk_record::Rootfs::ReadOnly;
        let mut cfg = boxdesk_supervisor::VmConfig::new(std::path::PathBuf::from("/r"), "true");
        cfg.net = boxdesk_supervisor::Net::Tsi;
        cfg.rootfs = boxdesk_supervisor::RootFs::Writable;
        lay_snapshot(&mut cfg, &snapshot);
        assert_eq!(
            cfg.net,
            boxdesk_supervisor::Net::None,
            "a closed snapshot must close a config that was open"
        );
        assert_eq!(cfg.rootfs, boxdesk_supervisor::RootFs::ReadOnly);
    }

    /// `--sound` and `--gpu` can only turn a posture on, so a snapshot that asked for one and a
    /// flag that asked for the other give a sandbox with both. There is no flag that means "not
    /// this time", so a union is the only honest reading.
    #[test]
    fn the_switches_a_flag_can_only_turn_on_are_unions() {
        let mut cfg = boxdesk_supervisor::VmConfig::new(std::path::PathBuf::from("/r"), "true");
        cfg.gpu = true; // as `--gpu` would have left it
        lay_snapshot(&mut cfg, &snapshot()); // which asks for sound, not the GPU
        assert!(cfg.gpu, "the flag's GPU should survive the snapshot");
        assert!(cfg.sound, "and the snapshot's sound card should arrive");
    }

    /// **Flag, then snapshot, then environment.** A flag is a choice about this command and a
    /// snapshot is one about this sandbox; the variable is the machine's air, and being asked for
    /// by name outranks it.
    #[test]
    fn a_limit_takes_the_flag_then_the_snapshot_then_the_air() {
        let four = std::num::NonZeroU8::new(4).expect("non-zero");
        let eight = std::num::NonZeroU8::new(8).expect("non-zero");
        assert_eq!(
            limit(Some(eight), Some(four), "BOXDESK_VCPUS_UNSET_FOR_THIS_TEST"),
            Ok(Some(eight)),
            "the flag wins over the snapshot"
        );
        assert_eq!(
            limit(None, Some(four), "BOXDESK_VCPUS_UNSET_FOR_THIS_TEST"),
            Ok(Some(four)),
            "the snapshot stands where no flag was given"
        );
        assert_eq!(
            limit::<std::num::NonZeroU8>(None, None, "BOXDESK_VCPUS_UNSET_FOR_THIS_TEST"),
            Ok(None),
            "with neither, nothing is said and the config's own default stands"
        );
    }

    /// A run with no command takes its snapshot's, and one with neither is refused by name.
    #[test]
    fn a_command_falls_back_to_the_snapshots_and_then_says_so() {
        let snapshot = snapshot();
        assert_eq!(
            command_for(&["echo".to_string()], Some(&snapshot)),
            Ok(vec!["echo".to_string()]),
            "a command given is the command run"
        );
        assert_eq!(
            command_for(&[], Some(&snapshot)),
            Ok(vec!["true".to_string()]),
            "and the snapshot's stands where none was"
        );
        let mut empty = snapshot;
        empty.command.clear();
        let err = command_for(&[], Some(&empty)).expect_err("neither said anything");
        assert!(
            err.contains("devbox"),
            "said {err} without naming the snapshot"
        );
        assert!(
            command_for(&[], None).is_err(),
            "and with no snapshot at all there is still nothing to run"
        );
    }

    /// Each posture's default is the closed one, and each crossing lands on the supervisor's own
    /// variant: the two mirrors meet here and nowhere else.
    #[test]
    fn every_posture_defaults_closed_and_crosses_to_its_own_variant() {
        assert_eq!(NetArg::default(), NetArg::None);
        assert_eq!(NetArg::None.into_net(), boxdesk_supervisor::Net::None);
        assert_eq!(NetArg::Tsi.into_net(), boxdesk_supervisor::Net::Tsi);
        assert_eq!(NetArg::None.tsi_flags(), 0, "no network asks for no hijack");
        assert_ne!(NetArg::Tsi.tsi_flags(), 0);

        assert_eq!(RootFsArg::default(), RootFsArg::ReadOnly);
        assert_eq!(
            RootFsArg::ReadOnly.into_rootfs(),
            boxdesk_supervisor::RootFs::ReadOnly
        );
        assert_eq!(
            RootFsArg::Writable.into_rootfs(),
            boxdesk_supervisor::RootFs::Writable
        );
        assert_eq!(
            RootFsArg::ReadOnly.into_access(),
            boxdesk_krun::FsAccess::ReadOnly
        );
        assert_eq!(
            RootFsArg::Writable.into_access(),
            boxdesk_krun::FsAccess::ReadWrite
        );
    }
}
