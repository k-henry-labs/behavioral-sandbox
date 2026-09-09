//! The icon font: Lucide, from a pinned upstream release, cut down by `cargo xtask icons` to the
//! glyphs named here, so the tree carries a few kilobytes of a font rather than the whole of one.
//!
//! - **A glyph is a `char` in a named font.** [`glyph`] draws one at a text size, in the text
//!   colour, so an icon scales and themes exactly as a word beside it does.
//! - **Drawn to one optical size.** Lucide draws a gear taller than a grid, so the same font size
//!   reads as two sizes in a row. Each icon carries the extent it is drawn to, and [`glyph`]
//!   corrects for it in a cell of the nominal width, which keeps every label at one left edge.
//! - **This file is the subset.** `cargo xtask icons` keeps the glyphs of every `\u{…}` literal
//!   below, and `every_icon_named_here_is_in_the_font` fails when one is named and the font was
//!   not cut again after.
//! - **The licence travels beside the font**: `fonts/LICENSE-lucide` (ISC).

/// The family the bundled font declares, which is how a `Text` asks for it.
pub(crate) const FONT: iced::Font = iced::Font::with_name("lucide");

/// The font itself, compiled in, so the binary carries its own icons wherever it is copied.
pub(crate) const BYTES: &[u8] = include_bytes!("../fonts/lucide.ttf");

/// One icon: the character it is in the font, and the extent its ink is drawn to, in the font's
/// own units, which is what [`glyph`] corrects for.
#[derive(Debug, Clone)]
pub(crate) struct Icon {
    ch: char,
    ink: u16,
}

/// The extent most of the set is drawn to, and so the one every icon is corrected to.
const INK: f32 = 833.0;

/// Names each icon once, as a const and as an entry of the test's list, so the two agree.
macro_rules! icons {
    ($($name:ident = $code:literal, $ink:literal;)*) => {
        $(pub(crate) const $name: Icon = Icon { ch: $code, ink: $ink };)*
        /// Every icon this build names, with its name, for the test below. `cargo xtask icons`
        /// reads the literals out of this file's text instead, so it needs no build of the app.
        #[cfg(test)]
        const ALL: [(&str, Icon); [$($code),*].len()] = [$((stringify!($name), $name)),*];
    };
}

icons! {
    PANEL_LEFT = '\u{e12a}', 832;
    GRID = '\u{e0ff}', 833;
    SQUARE_PLUS = '\u{e173}', 832;
    SETTINGS = '\u{e154}', 917;
    SUN_MOON = '\u{e2b2}', 876;
    SCALING = '\u{e2ec}', 832;
    ROCKET = '\u{e286}', 896;
    TERMINAL = '\u{e181}', 757;
    FOLDER = '\u{e0d7}', 918;
    CIRCLE_USER = '\u{e461}', 922;
    CLOSE = '\u{e1b2}', 582;
}

/// One icon at `size`, in the icon grey, drawn to the same optical size as every other and in a
/// cell of the nominal width, so a row of them shares one left edge.
pub(crate) fn glyph<'a>(icon: Icon, size: f32) -> iced::widget::Text<'a> {
    iced::widget::text(icon.ch.to_string())
        .font(FONT)
        .size(size * INK / f32::from(icon.ink))
        .width(size)
        .center()
        .style(|theme| iced::widget::text::Style {
            color: Some(crate::theme::icon(theme)),
        })
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
                face.glyph_index(icon.ch).is_some(),
                "{name} ({:?}) is not in fonts/lucide.ttf: run `cargo xtask icons`",
                icon.ch
            );
        }
    }

    /// Each icon's declared extent is the one the font draws it to, so [`glyph`] corrects by the
    /// right amount. Re-cutting from a release that redrew an icon fails here rather than
    /// silently drawing it a size out.
    #[test]
    fn every_icon_is_drawn_to_the_extent_it_declares() {
        let face = ttf_parser::Face::parse(BYTES, 0).expect("the bundled font parses");
        for (name, icon) in ALL {
            let id = face.glyph_index(icon.ch).expect("in the font");
            let box_ = face.glyph_bounding_box(id).expect("an inked glyph");
            let drawn = (box_.x_max - box_.x_min).max(box_.y_max - box_.y_min);
            assert_eq!(
                drawn,
                i16::try_from(icon.ink).expect("an extent in range"),
                "{name} is drawn to {drawn}, not the {} it declares",
                icon.ink
            );
        }
    }
}
