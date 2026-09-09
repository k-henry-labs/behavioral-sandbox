//! The text faces the window draws in: Inter for prose, Geist Mono for identifiers.
//!
//! - **The same families the web app serves**, so a window and a page read as one product. The
//!   web self-hosts them through `next/font`; here they are compiled in, cut by
//!   `cargo xtask fonts` to the characters the app draws.
//! - **A weight is a file.** `fontdb` matches a face by the weight it declares, so the two
//!   weights the app asks for are two faces per family, and [`SANS`] or [`MONO`] with
//!   `Weight::Semibold` is what picks the heavier one.
//! - **One family name, whatever the file is called.** Each `SemiBold` file names itself
//!   `Inter SemiBold` in the legacy field and `Inter` in the typographic one, which is the field
//!   a family is matched on; `each_face_declares_the_family_and_weight_the_app_asks_for` holds
//!   the cut to that.
//! - **Not the whole of Unicode.** Anything outside the cut falls back to a system face, which is
//!   what a guest printing a script the font never carried would do in any case.

use iced::Font;

/// Every face the app registers at startup, in no particular order: `fontdb` indexes them by what
/// they declare, not by the order they arrive.
pub(crate) const FACES: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
    include_bytes!("../fonts/GeistMono-Regular.ttf"),
    include_bytes!("../fonts/GeistMono-SemiBold.ttf"),
];

/// Prose: every label, heading and sentence in the window.
pub(crate) const SANS: Font = Font::with_name("Inter");

/// Identifiers, and only identifiers: a name, a command, a path, an id.
pub(crate) const MONO: Font = Font::with_name("Geist Mono");

#[cfg(test)]
mod tests {
    use super::*;

    /// Each cut face declares the family and weight the app matches on. A re-cut that dropped the
    /// typographic family would leave `Inter SemiBold` a family of its own, and every heading
    /// would quietly render at regular.
    #[test]
    fn each_face_declares_the_family_and_weight_the_app_asks_for() {
        // The typographic family (name id 16) where a face has one, else the legacy family (1):
        // the order `fontdb` reads them in.
        fn family_of(face: &ttf_parser::Face<'_>) -> String {
            let named = |id: u16| {
                face.names()
                    .into_iter()
                    .find(|n| n.name_id == id)
                    .and_then(|n| n.to_string())
            };
            named(16).or_else(|| named(1)).expect("a family name")
        }

        let want = [
            ("Inter", 400),
            ("Inter", 600),
            ("Geist Mono", 400),
            ("Geist Mono", 600),
        ];
        for (bytes, (family, weight)) in FACES.iter().zip(want) {
            let face = ttf_parser::Face::parse(bytes, 0).expect("a cut face parses");
            assert_eq!(family_of(&face), family, "the family a face is matched by");
            assert_eq!(face.weight().to_number(), weight, "{family}'s weight");
        }
        assert_eq!(
            Font::with_name(want[0].0),
            SANS,
            "the sans the app asks for is the family its faces declare"
        );
        assert_eq!(Font::with_name(want[2].0), MONO, "and the mono");
    }

    /// The cut answers every character the app itself draws, so no label falls back mid-line.
    /// A guest's own output is not covered, and is not this crate's to promise.
    #[test]
    fn every_character_the_app_draws_is_in_every_face() {
        // The non-ASCII of `screens.rs`: the mount arrow, the status dot, the separator, the
        // display `×`, an ellipsis and the title's chevron.
        let drawn = "abzABZ0189 /=-_.:@%\u{2190}\u{25CF}\u{00B7}\u{00D7}\u{2026}\u{203A}";
        for bytes in FACES {
            let face = ttf_parser::Face::parse(bytes, 0).expect("a cut face parses");
            for c in drawn.chars() {
                assert!(
                    face.glyph_index(c).is_some(),
                    "U+{:04X} is not in a cut face: re-run `cargo xtask fonts`",
                    c as u32
                );
            }
        }
    }
}
