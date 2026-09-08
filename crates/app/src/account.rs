//! The account this window is signed in to, and the one place a sign-in happens.
//!
//! - **Nothing in this window needs one.** Every sandbox it starts, lists and shows is local, and
//!   no part of that asks who you are. Signing in is beside the notebook, never in front of it:
//!   there is no screen a signed-out person cannot reach.
//! - **No sign-in has a service to reach.** [`begin`] is the one item here that reports that
//!   instead of doing it; the state, the row and the message loop around it are whole, so what a
//!   working sign-in changes is this function and the account it answers with.
//! - **Held for this launch only.** Nothing is written to disk: a credential belongs in the
//!   platform's own store, and this build has none to put one in.

/// Who this window is signed in as.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Account {
    /// Nobody, which is every launch so far.
    #[default]
    SignedOut,
    /// A sign-in is in flight, and the row is not pressable while it is.
    SigningIn,
    /// Signed in, under this address.
    SignedIn {
        /// What the account is named by, which is what the row shows.
        email: String,
    },
}

impl Account {
    /// What the account row reads in this state.
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::SignedOut => "Sign in",
            Self::SigningIn => "Signing in…",
            Self::SignedIn { email } => email,
        }
    }

    /// The quieter word beside the label, or `None` where the label says it all.
    pub(crate) fn hint(&self) -> Option<&'static str> {
        match self {
            Self::SignedOut | Self::SigningIn => None,
            Self::SignedIn { .. } => Some("Sign out"),
        }
    }
}

/// Signs in, answering with the address the account is named by.
///
/// **The one thing here that does not work**: there is no account service for it to reach. It
/// says so rather than returning quietly, because a button that did nothing reads as broken.
pub(crate) fn begin() -> Result<String, String> {
    Err("Signing in needs an account service to reach, and this build has none".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each state reads as itself in the row, and only a signed-in one offers the way back out.
    #[test]
    fn a_row_reads_the_state_it_is_in() {
        assert_eq!(Account::default(), Account::SignedOut);
        assert_eq!(Account::SignedOut.label(), "Sign in");
        assert_eq!(Account::SigningIn.label(), "Signing in…");
        let signed_in = Account::SignedIn {
            email: "someone@example.com".to_string(),
        };
        assert_eq!(signed_in.label(), "someone@example.com");
        assert_eq!(signed_in.hint(), Some("Sign out"));
        assert_eq!(Account::SignedOut.hint(), None);
        assert_eq!(Account::SigningIn.hint(), None);
    }

    /// A sign-in reports that it has nowhere to go, rather than answering with an account
    /// nobody authenticated. The window turns this into the line the operator reads.
    #[test]
    fn a_sign_in_with_no_service_says_so() {
        let why = begin().expect_err("there is no service to reach");
        assert!(why.contains("account service"), "{why}");
    }
}
