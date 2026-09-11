//! Which palette the notebook draws in: light, dark, or whichever the desktop is showing.
//!
//! - **Two palettes, Apple's.** Light and dark are the system palette in its two forms: the page,
//!   near-black or near-white text, and the system blue, green, orange and red. Every colour in
//!   `screens` comes from a semantic role on the extended palette, never from a literal.
//! - **`System` is the default** and follows the toolkit's report of the desktop's appearance, so
//!   the window flips when the desktop does. The other two hold still.
//! - **A name is refused, never guessed at.** A typo that silently fell back to the default would
//!   read as "that mode looks like the old one".

use std::fmt::Write as _;

/// What the notebook draws in, as a person picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    Light,
    #[default]
    Dark,
    System,
}

/// The name each mode prints and is asked for by.
impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "System",
        })
    }
}

/// Every mode the picker offers, in the order it offers them: the default first, then the two it
/// chooses between.
pub(crate) const MODES: [Mode; 3] = [Mode::System, Mode::Light, Mode::Dark];

/// The surface a card or a pane sits on, over the page: white in light, a step up in dark, as
/// macOS raises a group off the window under it.
pub(crate) fn raised(theme: &iced::Theme) -> iced::Color {
    if theme.extended_palette().is_dark {
        iced::Color::from_rgb8(0x12, 0x12, 0x12)
    } else {
        iced::Color::WHITE
    }
}

/// The sidebar's own surface, a step under the page rather than over it, which is the direction
/// macOS sets a source list against the pane beside it.
pub(crate) fn recessed(theme: &iced::Theme) -> iced::Color {
    if theme.extended_palette().is_dark {
        iced::Color::from_rgb8(0x05, 0x05, 0x05)
    } else {
        iced::Color::from_rgb8(0xEC, 0xEC, 0xEE)
    }
}

/// The step of grey a row wears while the pointer is on it.
pub(crate) fn hovered(theme: &iced::Theme) -> iced::Color {
    if theme.extended_palette().is_dark {
        iced::Color::from_rgb8(0x2F, 0x2F, 0x31)
    } else {
        iced::Color::from_rgb8(0xE1, 0xE1, 0xE6)
    }
}

/// The step a raised surface takes under the pointer: down in light, up in dark, because a card
/// is the lightest surface in one and a raised one in the other, so it cannot move the same way.
pub(crate) fn raised_hovered(theme: &iced::Theme) -> iced::Color {
    if theme.extended_palette().is_dark {
        iced::Color::from_rgb8(0x3A, 0x3A, 0x3C)
    } else {
        iced::Color::from_rgb8(0xE8, 0xE8, 0xED)
    }
}

/// The grey an icon is drawn in: the text a step back, as macOS sets a symbol beside a label,
/// so a row's icon does not outweigh its word.
pub(crate) fn icon(theme: &iced::Theme) -> iced::Color {
    theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.7)
}

/// The step of grey under what is picked: a source list's open row, a segment, a slider's track.
/// Neutral on purpose, since a generated ramp tints these towards the palette's own hue.
pub(crate) fn selected(theme: &iced::Theme) -> iced::Color {
    if theme.extended_palette().is_dark {
        iced::Color::from_rgb8(0x3A, 0x3A, 0x3C)
    } else {
        iced::Color::from_rgb8(0xD8, 0xD8, 0xDD)
    }
}

/// The theme `mode` draws in, given what the toolkit reports the desktop is showing.
pub(crate) fn theme(mode: Mode, desktop: iced::theme::Mode) -> iced::Theme {
    match (mode, desktop) {
        (Mode::Dark, _) | (Mode::System, iced::theme::Mode::Dark) => dark(),
        (Mode::Light, _) | (Mode::System, _) => light(),
    }
}

/// Apple's system palette in its light form: a white page and the system colours.
fn light() -> iced::Theme {
    iced::Theme::custom(
        "Light",
        iced::theme::Palette {
            background: iced::Color::from_rgb8(0xF2, 0xF2, 0xF7),
            text: iced::Color::from_rgb8(0x1D, 0x1D, 0x1F),
            primary: iced::Color::from_rgb8(0x00, 0x7A, 0xFF),
            success: iced::Color::from_rgb8(0x34, 0xC7, 0x59),
            warning: iced::Color::from_rgb8(0xFF, 0x95, 0x00),
            danger: iced::Color::from_rgb8(0xFF, 0x3B, 0x30),
        },
    )
}

/// The same palette in its dark form: the system's dark page, and the colours it brightens there.
fn dark() -> iced::Theme {
    iced::Theme::custom(
        "Dark",
        iced::theme::Palette {
            background: iced::Color::from_rgb8(0x0A, 0x0A, 0x0A),
            text: iced::Color::from_rgb8(0xEC, 0xEC, 0xEC),
            primary: iced::Color::from_rgb8(0xE8, 0x41, 0x42),
            success: iced::Color::from_rgb8(0x30, 0xD1, 0x58),
            warning: iced::Color::from_rgb8(0xFF, 0x9F, 0x0A),
            danger: iced::Color::from_rgb8(0xFF, 0x45, 0x3A),
        },
    )
}

/// The environment variable a mode can be named in, below the flag and above the default.
pub(crate) const ENV: &str = "BOXDESK_THEME";

/// The mode `asked` names, or the default when nothing asked.
///
/// Matching ignores case and anything that is not a letter or digit, so `System`, `system` and
/// ` SYSTEM ` are one name.
pub(crate) fn resolve(asked: Option<&str>) -> Result<Mode, String> {
    let Some(asked) = asked else {
        return Ok(Mode::default());
    };
    let wanted = normalise(asked);
    if wanted.is_empty() {
        return Err(refusal(asked));
    }
    MODES
        .into_iter()
        .find(|mode| normalise(&mode.to_string()) == wanted)
        .ok_or_else(|| refusal(asked))
}

/// The mode to open in, and a note when a saved name had to be let go.
///
/// An explicit ask (flag or env) is refused when unknown, as [`resolve`] refuses it; a stale
/// *saved* name only degrades to the default, because a launch should not be blocked by a file.
pub(crate) fn startup(
    asked: Option<&str>,
    saved: Option<&str>,
) -> Result<(Mode, Option<String>), String> {
    if asked.is_some() {
        return resolve(asked).map(|mode| (mode, None));
    }
    let Some(saved) = saved else {
        return Ok((Mode::default(), None));
    };
    match resolve(Some(saved)) {
        Ok(mode) => Ok((mode, None)),
        Err(_) => Ok((
            Mode::default(),
            Some(format!(
                "the saved theme {saved:?} is not one this build has; drawing in {}",
                Mode::default()
            )),
        )),
    }
}

/// The mode named on the command line, else in the environment, else none.
///
/// Takes the environment's value rather than reading it, so the precedence is a pure function and
/// its test needs neither `unsafe` nor a process-global the other tests race against.
pub(crate) fn asked_for(flag: Option<&str>, env: Option<String>) -> Option<String> {
    flag.map(str::to_owned)
        .or_else(|| env.filter(|v| !v.trim().is_empty()))
}

/// What the environment asks for, if anything.
pub(crate) fn from_env() -> Option<String> {
    std::env::var(ENV).ok()
}

/// A refusal that quotes every name it would have accepted, since the set is three.
fn refusal(asked: &str) -> String {
    let mut message = format!("no theme named {asked:?}. The ones there are:");
    for mode in MODES {
        let _ = write!(message, "\n  {mode}");
    }
    message
}

/// A name reduced to what distinguishes it: lowercase letters and digits.
fn normalise(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mode the picker offers can be asked for by the name it prints, and nothing asked is
    /// the default: the list is a promise the app keeps.
    #[test]
    fn every_mode_in_the_picker_can_be_named() {
        for mode in MODES {
            assert_eq!(
                resolve(Some(&mode.to_string())).expect("its own name"),
                mode
            );
        }
        assert_eq!(resolve(None).expect("nothing asked"), Mode::System);
    }

    /// `System` draws in whichever palette the desktop is showing; the other two hold still.
    #[test]
    fn system_follows_the_desktop_and_the_others_hold_still() {
        assert_eq!(theme(Mode::System, iced::theme::Mode::Dark), dark());
        assert_eq!(theme(Mode::System, iced::theme::Mode::Light), light());
        assert_eq!(
            theme(Mode::System, iced::theme::Mode::None),
            light(),
            "unknown is light"
        );
        assert_eq!(theme(Mode::Light, iced::theme::Mode::Dark), light());
        assert_eq!(theme(Mode::Dark, iced::theme::Mode::Light), dark());
    }

    /// The spelling a person actually types is accepted: the printed name, any casing, and the
    /// stray whitespace a shell history tends to carry.
    #[test]
    fn a_name_is_matched_however_it_is_spaced_and_cased() {
        for spelling in ["System", "system", "SYSTEM", "  system  "] {
            assert_eq!(
                resolve(Some(spelling)).expect("one mode, four spellings"),
                Mode::System,
                "{spelling:?}"
            );
        }
    }

    /// An unknown name is refused with the list, never quietly defaulted: a mode that silently
    /// did not change reads as a mode that looks like the old one.
    #[test]
    fn an_unknown_name_is_refused_and_says_what_would_have_worked() {
        let why = resolve(Some("drak")).expect_err("a typo is not a mode");
        assert!(why.contains("drak"), "names what was asked: {why}");
        assert!(why.contains("Dark"), "and what was meant: {why}");
        assert!(
            resolve(Some("")).is_err(),
            "an empty name is not the default"
        );
    }

    /// The flag outranks the environment, which outranks the default: the order the CLI's other
    /// knobs already use. An environment set to blanks is nothing asked, not a mode named "".
    #[test]
    fn the_flag_outranks_the_environment() {
        let env = || Some("Dark".to_string());
        assert_eq!(asked_for(Some("Light"), env()).as_deref(), Some("Light"));
        assert_eq!(asked_for(None, env()).as_deref(), Some("Dark"));
        assert_eq!(asked_for(None, None), None);
        assert_eq!(asked_for(None, Some("   ".to_string())), None);
    }

    /// A saved mode is used; a stale one (a palette an older build offered) degrades to the
    /// default with a note; an explicit ask is still refused when unknown, and outranks the saved.
    #[test]
    fn a_saved_mode_is_used_and_a_stale_one_degrades_with_a_note() {
        assert_eq!(
            startup(None, Some("Dark")).expect("a saved mode"),
            (Mode::Dark, None)
        );
        let (mode, note) = startup(None, Some("Nord")).expect("a stale name still opens");
        assert_eq!(mode, Mode::System);
        assert!(note.expect("with a note").contains("Nord"));
        startup(Some("drak"), Some("Dark")).expect_err("an explicit ask is refused");
        assert_eq!(
            startup(Some("Light"), Some("Dark")).expect("the ask outranks the saved"),
            (Mode::Light, None)
        );
        assert_eq!(
            startup(None, None).expect("nothing asked"),
            (Mode::System, None)
        );
    }
}
