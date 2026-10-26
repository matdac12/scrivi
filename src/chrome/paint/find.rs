//! Drawing the find bar (spec §4): variant E's `.find`. The input text is
//! the EDIT controls' own; this draws their frames, the toggle, the count and
//! the buttons around them.

use windows::core::Result;

use super::super::findbar::{Field, FindHit, FindLayout, Mode};
use super::{colour, rf, rounded, Pass};
use crate::chrome::Rect;

/// What one paint of the find bar shows.
pub struct FindView {
    pub layout: FindLayout,
    pub match_case: bool,
    /// "2 of 3".
    pub count: String,
    pub hover: Option<FindHit>,
    /// The input whose EDIT has the keyboard.
    pub focus: Option<Field>,
}

const FIELD_BORDER: u32 = 0xCFCFCB;
const INK: u32 = 0x2D4BA0;
const TOGGLE_ON_BACK: u32 = 0xE7ECF8;
const TOGGLE_ON_BORDER: u32 = 0xB9C5E6;
const TOGGLE_OFF_INK: u32 = 0x555555;
const COUNT_INK: u32 = 0x6B7079;
const GLYPH_INK: u32 = 0x444444;
const CLOSE_INK: u32 = 0x7A7F88;
const GLYPH_UP: &str = "\u{E70E}";
const GLYPH_DOWN: &str = "\u{E70D}";
const GLYPH_CLOSE: &str = "\u{E8BB}";
const FIELD_RADIUS: f32 = 5.0;
const TOGGLE_RADIUS: f32 = 4.0;

impl Pass<'_> {
    /// A 1 px outline, drawn just inside `r`.
    fn outline(&self, r: Rect, radius: f32, c: u32, width: f32) -> Result<()> {
        let h = width / 2.0;
        let mut shape = rf(r, self.oy);
        shape.left += h;
        shape.top += h;
        shape.right -= h;
        shape.bottom -= h;
        unsafe {
            self.rt.DrawRoundedRectangle(
                &rounded(shape, radius * self.scale - h),
                &self.brush(c, 1.0)?,
                width,
                None,
            )
        };
        Ok(())
    }

    /// An input's frame: white, with a hairline border; ink when it has the keyboard.
    fn field_frame(&self, r: Rect, focused: bool) -> Result<()> {
        self.fill_rounded(r, FIELD_RADIUS, 0xFFFFFF, 1.0)?;
        let hair = self.px(1.0).max(1) as f32;
        if focused {
            // The mockup's border plus a 1 px ring of the same ink.
            self.outline(r, FIELD_RADIUS, INK, hair * 2.0)
        } else {
            self.outline(r, FIELD_RADIUS, FIELD_BORDER, hair)
        }
    }

    /// A white button with a hairline border, shaded under the mouse.
    fn push_button(&self, r: Rect, label: &str, hot: bool) -> Result<()> {
        self.fill_rounded(r, TOGGLE_RADIUS, 0xFFFFFF, 1.0)?;
        if hot {
            self.fill_rounded(r, TOGGLE_RADIUS, 0x000000, super::HOVER_A)?;
        }
        self.outline(r, TOGGLE_RADIUS, FIELD_BORDER, self.px(1.0).max(1) as f32)?;
        self.text(label, &self.fonts.menu, r, colour::MENU_TEXT)
    }

    /// A glyph button without a frame, shaded under the mouse.
    fn glyph_button(&self, r: Rect, glyph: &str, small: bool, ink: u32, hot: bool) -> Result<()> {
        if hot {
            self.fill_rounded(r, TOGGLE_RADIUS, 0x000000, super::HOVER_A)?;
        }
        self.text(
            glyph,
            if small {
                &self.fonts.tab_glyph
            } else {
                &self.fonts.plus
            },
            r,
            ink,
        )
    }
}

pub(super) fn draw(p: &Pass, f: &FindView) -> Result<()> {
    let l = &f.layout;
    if l.mode == Mode::Hidden {
        return Ok(());
    }
    let hair = p.px(1.0).max(1);
    p.fill(
        Rect {
            top: l.band.bottom - hair,
            ..l.band
        },
        colour::HAIRLINE,
        1.0,
    )?;
    let hot = |hit: FindHit| f.hover == Some(hit);
    let focused = |field: Field| f.focus == Some(field);

    p.field_frame(
        l.field,
        focused(if l.mode == Mode::Goto {
            Field::Goto
        } else {
            Field::Find
        }),
    )?;
    p.glyph_button(l.close, GLYPH_CLOSE, true, CLOSE_INK, hot(FindHit::Close))?;
    if l.mode == Mode::Goto {
        return p.text("Go to line", &p.fonts.status, l.label, colour::MENU_TEXT);
    }

    // Aa: on, it is ink on a pale ink tint.
    if f.match_case {
        p.fill_rounded(l.match_case, TOGGLE_RADIUS, TOGGLE_ON_BACK, 1.0)?;
        p.outline(l.match_case, TOGGLE_RADIUS, TOGGLE_ON_BORDER, hair as f32)?;
    } else if hot(FindHit::MatchCase) {
        p.fill_rounded(l.match_case, TOGGLE_RADIUS, 0x000000, super::HOVER_A)?;
    }
    p.text(
        "Aa",
        &p.fonts.mono,
        l.match_case,
        if f.match_case { INK } else { TOGGLE_OFF_INK },
    )?;

    p.text(&f.count, &p.fonts.status, l.count, COUNT_INK)?;
    p.glyph_button(l.prev, GLYPH_UP, false, GLYPH_INK, hot(FindHit::Prev))?;
    p.glyph_button(l.next, GLYPH_DOWN, false, GLYPH_INK, hot(FindHit::Next))?;

    if l.mode == Mode::Replace {
        p.field_frame(l.replace_field, focused(Field::Replace))?;
        p.push_button(l.replace, "Replace", hot(FindHit::Replace))?;
        p.push_button(l.replace_all, "Replace all", hot(FindHit::ReplaceAll))?;
    }
    Ok(())
}
