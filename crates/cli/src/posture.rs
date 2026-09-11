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

#[cfg(test)]
mod tests {
    use super::*;

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
