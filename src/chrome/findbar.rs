//! The find bar's layout and hit test (spec §4, §11): pure, in client
//! pixels, from the band `chrome::Layout` reserves under the title row.
//! Sizes in DIPs are the mockup's `.find`: 6 px padding, 26 px fields, 8 px gaps.
//! The input fields are Win32 EDIT controls; this only says where they go.

use super::Rect;

/// What the band shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    /// Find field, Aa, "n of m", ↑ ↓, ×.
    Find,
    /// Find, plus the replace row.
    Replace,
    /// Go to line: a small input in the same place.
    Goto,
}

/// Which input a key or click is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Find,
    Replace,
    Goto,
}

/// What a point in the band is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindHit {
    /// An input's frame: the EDIT control inside it answers its own clicks.
    Field(Field),
    MatchCase,
    Prev,
    Next,
    Close,
    Replace,
    ReplaceAll,
    /// The band, but nothing in it.
    None,
}

const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 6.0;
const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 6.0;
const GAP: f32 = 8.0;
const FIELD_W: f32 = 300.0;
const FIELD_MIN_W: f32 = 120.0;
const GOTO_W: f32 = 140.0;
const GOTO_LABEL_W: f32 = 70.0;
const TOGGLE_W: f32 = 30.0;
const TOGGLE_H: f32 = 22.0;
const COUNT_W: f32 = 84.0;
const ICON_BUTTON: f32 = 26.0;
const REPLACE_W: f32 = 72.0;
const REPLACE_ALL_W: f32 = 92.0;
/// Room the EDIT leaves inside its frame: text inset, and top and bottom.
const EDIT_INSET_X: f32 = 8.0;
const EDIT_INSET_Y: f32 = 4.0;

/// The band's height in DIPs: 0 when hidden, one row, or two with Replace.
pub fn height(mode: Mode) -> f32 {
    match mode {
        Mode::Hidden => 0.0,
        Mode::Find | Mode::Goto => ROW_H + 2.0 * PAD_Y,
        Mode::Replace => 2.0 * ROW_H + ROW_GAP + 2.0 * PAD_Y,
    }
}

/// Where everything in the band is, for one mode.
#[derive(Clone, Debug, PartialEq)]
pub struct FindLayout {
    pub mode: Mode,
    pub band: Rect,
    /// The frame of the find field (or, in Go to line, of its input).
    pub field: Rect,
    /// Go to line: the "Go to line" label before the input.
    pub label: Rect,
    pub match_case: Rect,
    pub count: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub close: Rect,
    pub replace_field: Rect,
    pub replace: Rect,
    pub replace_all: Rect,
    /// DIPs to pixels, for the EDIT inset.
    scale: f32,
}

impl FindLayout {
    pub fn new(band: Rect, scale: f32, mode: Mode) -> FindLayout {
        let px = |dip: f32| (dip * scale).round() as i32;
        let none = Rect { left: band.left, top: band.top, right: band.left, bottom: band.top };
        let (pad_x, gap, row_h) = (px(PAD_X), px(GAP), px(ROW_H));
        let row1_t = band.top + px(PAD_Y);
        let row2_t = row1_t + row_h + px(ROW_GAP);
        let rect = |left: i32, top: i32, w: i32, h: i32| Rect { left, top, right: left + w.max(0), bottom: top + h };
        let centred = |left: i32, row_t: i32, w: f32, h: f32| rect(left, row_t + (row_h - px(h)) / 2, px(w), px(h));

        let close = rect(band.right - pad_x - px(ICON_BUTTON), row1_t, px(ICON_BUTTON), row_h);
        let mut l = FindLayout {
            mode,
            band,
            field: none,
            label: none,
            match_case: none,
            count: none,
            prev: none,
            next: none,
            close,
            replace_field: none,
            replace: none,
            replace_all: none,
            scale,
        };
        match mode {
            Mode::Hidden => l.close = none,
            Mode::Goto => {
                l.label = rect(pad_x, row1_t, px(GOTO_LABEL_W), row_h);
                l.field = rect(l.label.right, row1_t, px(GOTO_W), row_h);
            }
            Mode::Find | Mode::Replace => {
                // Right to left from ×; the field takes what is left, up to its width.
                let next = rect(close.left - gap - px(ICON_BUTTON), row1_t, px(ICON_BUTTON), row_h);
                let prev = rect(next.left - gap - px(ICON_BUTTON), row1_t, px(ICON_BUTTON), row_h);
                let count = rect(prev.left - gap - px(COUNT_W), row1_t, px(COUNT_W), row_h);
                let toggle = centred(count.left - gap - px(TOGGLE_W), row1_t, TOGGLE_W, TOGGLE_H);
                let room = toggle.left - gap - pad_x;
                l.field = rect(pad_x, row1_t, room.clamp(px(FIELD_MIN_W), px(FIELD_W)), row_h);
                (l.next, l.prev, l.count, l.match_case) = (next, prev, count, toggle);
                if mode == Mode::Replace {
                    l.replace_field = rect(pad_x, row2_t, l.field.right - pad_x, row_h);
                    l.replace = rect(l.replace_field.right + gap, row2_t, px(REPLACE_W), row_h);
                    l.replace_all = rect(l.replace.right + gap, row2_t, px(REPLACE_ALL_W), row_h);
                }
            }
        }
        l
    }

    /// Where an input's EDIT control goes: inside its frame, one line tall.
    pub fn edit_rect(&self, field: Field) -> Rect {
        let frame = match field {
            Field::Find | Field::Goto => self.field,
            Field::Replace => self.replace_field,
        };
        let (x, y) = ((EDIT_INSET_X * self.scale).round() as i32, (EDIT_INSET_Y * self.scale).round() as i32);
        Rect { left: frame.left + x, top: frame.top + y, right: (frame.right - x).max(frame.left + x), bottom: frame.bottom - y }
    }

    pub fn hit_test(&self, x: i32, y: i32) -> FindHit {
        let at = |r: &Rect| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let pieces = match self.mode {
            Mode::Hidden => return FindHit::None,
            Mode::Goto => vec![(self.close, FindHit::Close), (self.field, FindHit::Field(Field::Goto))],
            Mode::Find | Mode::Replace => vec![
                (self.close, FindHit::Close),
                (self.next, FindHit::Next),
                (self.prev, FindHit::Prev),
                (self.match_case, FindHit::MatchCase),
                (self.field, FindHit::Field(Field::Find)),
                (self.replace_field, FindHit::Field(Field::Replace)),
                (self.replace, FindHit::Replace),
                (self.replace_all, FindHit::ReplaceAll),
            ],
        };
        pieces.into_iter().find(|(r, _)| at(r)).map_or(FindHit::None, |(_, hit)| hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(width: i32, mode: Mode, scale: f32) -> Rect {
        Rect { left: 0, top: 42, right: width, bottom: 42 + (height(mode) * scale).round() as i32 }
    }

    fn layout(width: i32, mode: Mode) -> FindLayout {
        FindLayout::new(band(width, mode, 1.0), 1.0, mode)
    }

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }

    #[test]
    fn heights_follow_the_mode() {
        assert_eq!(height(Mode::Hidden), 0.0);
        assert_eq!(height(Mode::Find), 38.0);
        assert_eq!(height(Mode::Goto), 38.0);
        assert_eq!(height(Mode::Replace), 70.0);
    }

    #[test]
    fn the_find_row_follows_the_mockup() {
        let l = layout(1000, Mode::Find);
        // 6 px padding, 26 px row, 12 px sides.
        assert_eq!(l.field, r(12, 48, 312, 74));
        assert_eq!(l.close, r(962, 48, 988, 74));
        assert_eq!(l.next, r(928, 48, 954, 74));
        assert_eq!(l.prev, r(894, 48, 920, 74));
        assert_eq!(l.count, r(802, 48, 886, 74));
        // The toggle is centred in the row.
        assert_eq!(l.match_case, r(764, 50, 794, 72));
        // Nothing of the replace row.
        assert_eq!(l.replace_field.right, l.replace_field.left);
    }

    #[test]
    fn the_replace_row_sits_under_the_find_field() {
        let l = layout(1000, Mode::Replace);
        assert_eq!(l.replace_field, r(12, 80, 312, 106));
        assert_eq!(l.replace, r(320, 80, 392, 106));
        assert_eq!(l.replace_all, r(400, 80, 492, 106));
        assert_eq!(l.band.bottom - l.band.top, 70);
    }

    #[test]
    fn the_field_shrinks_in_a_narrow_window() {
        let l = layout(480, Mode::Replace);
        assert!(l.field.right - l.field.left < 300);
        assert!(l.field.right - l.field.left >= 120);
        assert!(l.field.right + 8 <= l.match_case.left);
        assert_eq!(l.replace_field.right, l.field.right);
    }

    #[test]
    fn go_to_line_is_a_label_and_a_small_input() {
        let l = layout(1000, Mode::Goto);
        assert_eq!(l.label, r(12, 48, 82, 74));
        assert_eq!(l.field, r(82, 48, 222, 74));
        assert_eq!(l.close, r(962, 48, 988, 74));
    }

    #[test]
    fn the_edit_sits_inside_its_frame() {
        let l = layout(1000, Mode::Replace);
        assert_eq!(l.edit_rect(Field::Find), r(20, 52, 304, 70));
        assert_eq!(l.edit_rect(Field::Replace), r(20, 84, 304, 102));
        let g = layout(1000, Mode::Goto);
        assert_eq!(g.edit_rect(Field::Goto), r(90, 52, 214, 70));
    }

    #[test]
    fn hit_test_names_each_piece() {
        let l = layout(1000, Mode::Replace);
        assert_eq!(l.hit_test(100, 60), FindHit::Field(Field::Find));
        assert_eq!(l.hit_test(100, 90), FindHit::Field(Field::Replace));
        assert_eq!(l.hit_test(770, 60), FindHit::MatchCase);
        assert_eq!(l.hit_test(900, 60), FindHit::Prev);
        assert_eq!(l.hit_test(940, 60), FindHit::Next);
        assert_eq!(l.hit_test(970, 60), FindHit::Close);
        assert_eq!(l.hit_test(350, 90), FindHit::Replace);
        assert_eq!(l.hit_test(450, 90), FindHit::ReplaceAll);
        assert_eq!(l.hit_test(600, 60), FindHit::None);
        assert_eq!(l.hit_test(600, 90), FindHit::None);
    }

    #[test]
    fn hit_test_follows_the_mode() {
        // The replace row is not there in Find.
        assert_eq!(layout(1000, Mode::Find).hit_test(350, 90), FindHit::None);
        assert_eq!(layout(1000, Mode::Goto).hit_test(100, 60), FindHit::Field(Field::Goto));
        assert_eq!(layout(1000, Mode::Goto).hit_test(900, 60), FindHit::None);
        assert_eq!(layout(1000, Mode::Hidden).hit_test(100, 60), FindHit::None);
    }

    #[test]
    fn it_scales_with_the_dpi() {
        let m = Mode::Replace;
        let l = FindLayout::new(band(1500, m, 1.5), 1.5, m);
        assert_eq!(l.field.bottom - l.field.top, 39);
        assert_eq!(l.field.left, 18);
        assert_eq!(l.field.top, 42 + 9);
        assert_eq!(l.replace_field.top, 42 + 9 + 39 + 9);
        assert_eq!(l.band.bottom - l.band.top, 105);
    }
}
