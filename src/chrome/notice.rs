//! The Notice bar (spec §4): a full-width band under the title row with a
//! message, buttons (at most one primary) and an optional ×.
//!
//! A Notice is data ([`Spec`]); [`for_tab`] makes one from a Tab's
//! [`tabs::Notice`], and [`Parts`] places its pieces for hit testing. Both are
//! pure. [`draw`] paints it.

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::paint::{colour, Pass, View, GLYPH_CLOSE, SMALL_RADIUS};
use super::Rect;
use crate::tabs::{self, NoticeAction};

/// Height of the band in DIPs.
pub const HEIGHT: f32 = 38.0;
const PAD_L: f32 = 16.0;
const PAD_R: f32 = 8.0;
const GAP: f32 = 8.0;
const BUTTON_H: f32 = 26.0;
const BUTTON_PAD: f32 = 12.0;
const CLOSE: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Something to know.
    Amber,
    /// Something is wrong.
    Rose,
}

/// A run of the message; `strong` is the emphasised file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub strong: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub primary: bool,
    /// What a click asks the Tab to do.
    pub action: NoticeAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: Kind,
    pub message: Vec<Span>,
    /// Left to right.
    pub buttons: Vec<Button>,
    /// Shows the ×, which sends [`NoticeAction::Dismiss`].
    pub closable: bool,
}

/// What a click landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Button(usize),
    Close,
}

/// Where the pieces of a Notice sit in its band, in client pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    pub message: Rect,
    pub buttons: Vec<Rect>,
    pub close: Option<Rect>,
}

impl Parts {
    /// `label_widths` are the buttons' text widths in DIPs, left to right.
    pub fn new(band: Rect, scale: f32, label_widths: &[f32], closable: bool) -> Parts {
        let px = |dip: f32| (dip * scale).round() as i32;
        let centred = |left: i32, right: i32, height: f32| {
            let top = band.top + (band.bottom - band.top - px(height)) / 2;
            Rect { left, top, right: right.max(left), bottom: top + px(height) }
        };
        let mut right = band.right - px(PAD_R);
        let close = closable.then(|| {
            let r = centred(right - px(CLOSE), right, CLOSE);
            right = r.left;
            r
        });
        let mut buttons: Vec<Rect> = label_widths
            .iter()
            .rev()
            .map(|w| {
                if right < band.right - px(PAD_R) || close.is_some() {
                    right -= px(GAP);
                }
                let r = centred(right - px(w + 2.0 * BUTTON_PAD), right, BUTTON_H);
                right = r.left;
                r
            })
            .collect();
        buttons.reverse();
        let left = band.left + px(PAD_L);
        let message = Rect { left, top: band.top, right: (right - px(GAP)).max(left), bottom: band.bottom };
        Parts { message, buttons, close }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let at = |r: &Rect| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if self.close.as_ref().is_some_and(at) {
            return Some(Hit::Close);
        }
        self.buttons.iter().position(at).map(Hit::Button)
    }
}

impl Spec {
    /// The action a click on `hit` asks for.
    pub fn action(&self, hit: Hit) -> Option<NoticeAction> {
        match hit {
            Hit::Close => self.closable.then_some(NoticeAction::Dismiss),
            Hit::Button(i) => self.buttons.get(i).map(|b| b.action),
        }
    }
}

/// "212 MB": whole MiB, rounded.
fn megabytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    format!("{} MB", (bytes + MIB / 2) / MIB)
}

fn plain(text: &str) -> Span {
    Span { text: text.to_string(), strong: false }
}

fn strong(text: &str) -> Span {
    Span { text: text.to_string(), strong: true }
}

fn button(label: &str, primary: bool, action: NoticeAction) -> Button {
    Button { label: label.to_string(), primary, action }
}

/// The Notice a Tab shows (spec §4): its `notice`, the file name and size.
pub fn for_tab(notice: tabs::Notice, name: &str, size: Option<u64>) -> Spec {
    match notice {
        tabs::Notice::DiskChange => Spec {
            kind: Kind::Amber,
            message: vec![strong(name), plain(" changed on disk while you had unsaved edits.")],
            buttons: vec![
                button("Keep mine", false, NoticeAction::KeepMine),
                button("Reload", true, NoticeAction::Reload),
            ],
            closable: false,
        },
        tabs::Notice::DeletedOnDisk => Spec {
            kind: Kind::Rose,
            message: vec![
                strong(name),
                plain(" was deleted or renamed on disk. Your text is still here; saving writes the file again."),
            ],
            buttons: vec![button("Save", true, NoticeAction::Save)],
            closable: false,
        },
        tabs::Notice::LargeFile => {
            let size = size.map_or(String::new(), |s| format!(" ({})", megabytes(s)));
            Spec {
                kind: Kind::Amber,
                message: vec![plain(&format!("Large file{size}: read-only, without wrap or styling."))],
                buttons: Vec::new(),
                closable: true,
            }
        }
    }
}

/// Paint the band `parts` was placed in. `measure` gives a span's width in pixels.
pub(super) fn draw(p: &Pass, v: &View, spec: &Spec, parts: &Parts, measure: &dyn Fn(&str, bool) -> f32) -> Result<()> {
    let band = v.layout.notice;
    let (back, border, ink) = match spec.kind {
        Kind::Amber => (colour::AMBER_BACK, colour::AMBER_BORDER, colour::AMBER_TEXT),
        Kind::Rose => (colour::ROSE_BACK, colour::ROSE_BORDER, colour::ROSE_TEXT),
    };
    p.fill(band, back, 1.0)?;
    let hair = p.px(1.0).max(1);
    p.fill(Rect { top: band.bottom - hair, ..band }, border, 1.0)?;

    let mut x = parts.message.left;
    for span in &spec.message {
        let width = measure(&span.text, span.strong).ceil() as i32;
        let r = Rect { left: x, right: (x + width).min(parts.message.right), ..parts.message };
        if r.right <= r.left {
            break;
        }
        let font = if span.strong { &p.fonts.notice_strong } else { &p.fonts.notice };
        p.text(&span.text, font, r, ink)?;
        x += width;
    }

    for (r, b) in parts.buttons.iter().zip(&spec.buttons) {
        if b.primary {
            p.fill_rounded(*r, SMALL_RADIUS, colour::INK, 1.0)?;
            p.text(&b.label, &p.fonts.notice_button, *r, 0xFFFFFF)?;
        } else {
            let edge = D2D_RECT_F {
                left: r.left as f32 + 0.5,
                top: (r.top - p.oy) as f32 + 0.5,
                right: r.right as f32 - 0.5,
                bottom: (r.bottom - p.oy) as f32 - 0.5,
            };
            unsafe {
                p.rt.DrawRoundedRectangle(
                    &super::paint::rounded(edge, SMALL_RADIUS * p.scale),
                    &p.brush(ink, 0.45)?,
                    1.0,
                    None,
                );
            }
            p.text(&b.label, &p.fonts.notice_button, *r, ink)?;
        }
    }
    if let Some(close) = parts.close {
        p.text(GLYPH_CLOSE, &p.fonts.tab_glyph, close, ink)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band() -> Rect {
        Rect { left: 0, top: 42, right: 1000, bottom: 80 }
    }

    #[test]
    fn the_large_file_notice_says_the_size_and_closes_with_an_x() {
        let spec = for_tab(tabs::Notice::LargeFile, "big.log", Some(212 * 1024 * 1024));
        assert_eq!(spec.kind, Kind::Amber);
        assert_eq!(spec.message, [plain("Large file (212 MB): read-only, without wrap or styling.")]);
        assert!(spec.buttons.is_empty());
        assert_eq!(spec.action(Hit::Close), Some(NoticeAction::Dismiss));
    }

    #[test]
    fn a_size_just_over_50_mb_reads_50_mb() {
        let spec = for_tab(tabs::Notice::LargeFile, "a", Some(50 * 1024 * 1024 + 1));
        assert!(spec.message[0].text.contains("(50 MB)"));
    }

    #[test]
    fn disk_change_has_keep_mine_and_a_primary_reload() {
        let spec = for_tab(tabs::Notice::DiskChange, "notes.md", None);
        assert_eq!(spec.kind, Kind::Amber);
        assert_eq!(spec.message[0], strong("notes.md"));
        let labels: Vec<_> = spec.buttons.iter().map(|b| (b.label.as_str(), b.primary)).collect();
        assert_eq!(labels, [("Keep mine", false), ("Reload", true)]);
        assert!(!spec.closable);
        assert_eq!(spec.action(Hit::Button(1)), Some(NoticeAction::Reload));
        assert_eq!(spec.action(Hit::Close), None);
    }

    #[test]
    fn deleted_on_disk_is_rose_with_a_primary_save() {
        let spec = for_tab(tabs::Notice::DeletedOnDisk, "notes.md", None);
        assert_eq!(spec.kind, Kind::Rose);
        assert_eq!(spec.action(Hit::Button(0)), Some(NoticeAction::Save));
        assert!(spec.buttons[0].primary);
    }

    #[test]
    fn the_x_sits_at_the_right_edge_and_buttons_before_it() {
        let parts = Parts::new(band(), 1.0, &[50.0, 40.0], true);
        let close = parts.close.unwrap();
        assert_eq!(close.right, 1000 - 8);
        assert!(parts.buttons[1].right < close.left);
        assert!(parts.buttons[0].right < parts.buttons[1].left);
        assert_eq!(parts.buttons[1].right - parts.buttons[1].left, 40 + 24);
        assert!(parts.message.right < parts.buttons[0].left);
    }

    #[test]
    fn without_an_x_the_last_button_ends_at_the_padding() {
        let parts = Parts::new(band(), 1.0, &[40.0], false);
        assert_eq!(parts.close, None);
        assert_eq!(parts.buttons[0].right, 1000 - 8);
    }

    #[test]
    fn hit_test_finds_the_x_and_each_button() {
        let parts = Parts::new(band(), 1.0, &[50.0, 40.0], true);
        let centre = |r: Rect| ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        let (x, y) = centre(parts.close.unwrap());
        assert_eq!(parts.hit(x, y), Some(Hit::Close));
        for i in 0..2 {
            let (x, y) = centre(parts.buttons[i]);
            assert_eq!(parts.hit(x, y), Some(Hit::Button(i)));
        }
        assert_eq!(parts.hit(parts.message.left + 1, 60), None);
    }

    #[test]
    fn pieces_fit_inside_the_band_at_150_percent() {
        let parts = Parts::new(Rect { left: 0, top: 63, right: 1500, bottom: 120 }, 1.5, &[60.0], true);
        for r in parts.buttons.iter().chain(parts.close.iter()) {
            assert!(r.top >= 63 && r.bottom <= 120);
        }
    }
}
