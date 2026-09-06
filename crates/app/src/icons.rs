//! The icon font: Lucide, from a pinned upstream release, cut down by `cargo xtask icons` to the
//! glyphs named here, so the tree carries a few kilobytes of a font rather than the whole of one.
//!
//! - **A glyph is a `char` in a named font.** [`glyph`] draws one at a text size, in the text
//!   colour, so an icon scales and themes exactly as a word beside it does.
//! - **This file is the subset.** `cargo xtask icons` keeps the glyphs of every `\u{…}` literal
//!   below, and `every_icon_named_here_is_in_the_font` fails when one is named and the font was
//!   not cut again after.
//! - **The licence travels beside the font**: `fonts/LICENSE-lucide` (ISC).

/// The family the bundled font declares, which is how a `Text` asks for it.
pub(crate) const FONT: iced::Font = iced::Font::with_name("lucide");

/// The font itself, compiled in, so the binary carries its own icons wherever it is copied.
pub(crate) const BYTES: &[u8] = include_bytes!("../fonts/lucide.ttf");

/// Names each icon once, as a const and as an entry of the test's list, so the two agree.
macro_rules! icons {
    ($($name:ident = $code:literal;)*) => {
        $(pub(crate) const $name: char = $code;)*
        /// Every icon this build names, with its name, for the test below. `cargo xtask icons`
        /// reads the literals out of this file's text instead, so it needs no build of the app.
        #[cfg(test)]
        const ALL: [(&str, char); [$($code),*].len()] = [$((stringify!($name), $code)),*];
    };
}

icons! {
    PANEL_LEFT = '\u{e12a}';
    GRID = '\u{e0ff}';
    SQUARE_PLUS = '\u{e173}';
    SETTINGS = '\u{e154}';
    SUN_MOON = '\u{e2b2}';
    SCALING = '\u{e2ec}';
    ROCKET = '\u{e286}';
    TERMINAL = '\u{e181}';
    FOLDER = '\u{e0d7}';
}

/// One icon at `size`, in the text colour, sized to sit beside a label of that size.
pub(crate) fn glyph<'a>(icon: char, size: f32) -> iced::widget::Text<'a> {
    iced::widget::text(icon.to_string()).font(FONT).size(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An icon named here and missing from the font would draw as a box; the cut is only as wide
    /// as this list, so the check is that the two agree.
    ///
    /// It is also what holds the cut to a font the app can read at all: `glyph_index` is a `cmap`
    /// lookup, so a subsetter that dropped that table would fail every line of this.
    #[test]
    fn every_icon_named_here_is_in_the_font() {
        let face = ttf_parser::Face::parse(BYTES, 0).expect("the bundled font parses");
        for (name, icon) in ALL {
            assert!(
                face.glyph_index(icon).is_some(),
                "{name} ({icon:?}) is not in fonts/lucide.ttf: run `cargo xtask icons`"
            );
        }
    }
}
