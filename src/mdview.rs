//! Markdown styling in the Source view (spec §5): the Scintilla styles from the
//! palette, and `mdstyle`'s runs written into a document. Every style keeps the
//! font size, so every line keeps its height.

use std::collections::HashMap;
use std::hash::Hash;

use crate::editor::sci::*;
use crate::editor::{rgb, Editor};
use crate::mdstyle::{self, Style};

// Scintilla style numbers; 0..=31 are ours (32 and up are Scintilla's own).
const S_PLAIN: u8 = 0;
const S_MARKER: u8 = 1;
/// H1..H6 are 2..=7.
const S_HEADING: u8 = 2;
const S_BOLD: u8 = 8;
const S_ITALIC: u8 = 9;
const S_BOLD_ITALIC: u8 = 10;
const S_CODE: u8 = 11;
const S_BLOCK: u8 = 13;
const S_LINK: u8 = 15;
const S_QUOTE: u8 = 16;
const S_STRIKE: u8 = 19;
const S_TASK_DONE: u8 = 21;
const S_URL: u8 = 22;

/// The indicator that draws the line through struck-out text.
const INDIC_STRIKE_ID: usize = 8;

// The §4 palette, and the prototype's inks where the palette has none.
const INK: u32 = 0x2D4BA0;
const PENCIL: u32 = 0xA4A8B0;
const CODE_TINT: u32 = 0xF3F1EC;
const GRAPHITE: u32 = 0x1F2328;
const MUTED: u32 = 0x7D7A72;
const CODE_INK: u32 = 0x9E3B2C;
const GREEN: u32 = 0x3E8E5E;
const LINK_INK: u32 = 0x2C62D6;
const URL_INK: u32 = 0x6F8FD8;
const STRIKE_INK: u32 = 0xA09C94;
/// H1 is `INK`; the others get lighter.
const HEADING_INK: [u32; 6] = [INK, 0x3D5CB5, 0x5470C0, 0x6578B8, 0x6578B8, 0x6578B8];

/// The Scintilla style a run is drawn in.
pub fn style_number(style: Style) -> u8 {
    match style {
        Style::Plain => S_PLAIN,
        Style::Heading(level) => S_HEADING + level.clamp(1, 6) - 1,
        Style::Bold => S_BOLD,
        Style::Italic => S_ITALIC,
        Style::BoldItalic => S_BOLD_ITALIC,
        Style::Strike => S_STRIKE,
        Style::InlineCode => S_CODE,
        Style::CodeBlock => S_BLOCK,
        Style::Link => S_LINK,
        Style::Url => S_URL,
        Style::Quote => S_QUOTE,
        Style::TaskDone => S_TASK_DONE,
        Style::Marker => S_MARKER,
    }
}

/// `mdstyle` runs as consecutive `(length, style number)` segments that cover
/// `0..len`, with the gaps between runs plain.
fn segments(len: usize, runs: &[(std::ops::Range<usize>, Style)]) -> Vec<(usize, u8)> {
    let mut out = Vec::with_capacity(runs.len() * 2 + 1);
    let mut at = 0;
    for (range, style) in runs {
        let (start, end) = (range.start.min(len), range.end.min(len));
        if start > at {
            out.push((start - at, S_PLAIN));
        }
        if end > start.max(at) {
            out.push((end - start.max(at), style_number(*style)));
        }
        at = at.max(end);
    }
    if at < len {
        out.push((len - at, S_PLAIN));
    }
    out
}

/// What to do about the shown Tab's styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Nothing,
    Apply,
    Clear,
}

/// Which Tabs' documents hold applied styles (`K` is `TabId`), and which of
/// those have been edited since.
pub struct Applied<K>(HashMap<K, bool>);

impl<K> Default for Applied<K> {
    fn default() -> Self {
        Applied(HashMap::new())
    }
}

impl<K: Copy + Eq + Hash> Applied<K> {
    /// The text of Tab `id` changed: its styles are stale, whichever Tab is
    /// shown when the restyle timer fires.
    pub fn touch(&mut self, id: K) {
        if let Some(stale) = self.0.get_mut(&id) {
            *stale = true;
        }
    }

    /// Decide for Tab `id`; `wanted` is `Tab::styled`. Records the outcome.
    pub fn plan(&mut self, id: K, wanted: bool) -> Plan {
        if wanted {
            if self.0.insert(id, false) != Some(false) {
                Plan::Apply
            } else {
                Plan::Nothing
            }
        } else if self.0.remove(&id).is_some() {
            Plan::Clear
        } else {
            Plan::Nothing
        }
    }

    /// The Tab's document was replaced or dropped: nothing is applied to it.
    pub fn forget(&mut self, id: K) {
        self.0.remove(&id);
    }
}

/// Define the styles. Call after the editor's own look, whose
/// `SCI_STYLECLEARALL` would wipe them.
pub fn define(e: &Editor) {
    let fore = |s: u8, c: u32| e.call(SCI_STYLESETFORE, s as usize, rgb(c));
    let back = |s: u8, c: u32| e.call(SCI_STYLESETBACK, s as usize, rgb(c));
    let weight = |s: u8, w: isize| e.call(SCI_STYLESETWEIGHT, s as usize, w);
    let italic = |s: u8| e.call(SCI_STYLESETITALIC, s as usize, 1);
    let eol = |s: u8| e.call(SCI_STYLESETEOLFILLED, s as usize, 1);

    fore(S_MARKER, PENCIL);
    for (i, ink) in HEADING_INK.into_iter().enumerate() {
        let s = S_HEADING + i as u8;
        fore(s, ink);
        weight(s, if i < 3 { 700 } else { 600 });
    }
    weight(S_BOLD, 700);
    italic(S_ITALIC);
    weight(S_BOLD_ITALIC, 700);
    italic(S_BOLD_ITALIC);
    fore(S_CODE, CODE_INK);
    back(S_CODE, CODE_TINT);
    fore(S_BLOCK, GRAPHITE);
    back(S_BLOCK, CODE_TINT);
    // A fence's tint runs to the edge of the line.
    eol(S_BLOCK);
    fore(S_LINK, LINK_INK);
    fore(S_URL, URL_INK);
    fore(S_QUOTE, MUTED);
    italic(S_QUOTE);
    fore(S_STRIKE, STRIKE_INK);
    fore(S_TASK_DONE, GREEN);
    weight(S_TASK_DONE, 700);

    e.call(SCI_INDICSETSTYLE, INDIC_STRIKE_ID, INDIC_STRIKE);
    e.call(SCI_INDICSETFORE, INDIC_STRIKE_ID, rgb(STRIKE_INK));
}

/// Style the shown document from `mdstyle`'s runs over its whole text.
pub fn apply(e: &Editor) {
    let len = e.text_len();
    let ptr = e.call(SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
    if ptr.is_null() {
        return;
    }
    // SAFETY: Scintilla's contiguous text, `len` bytes, valid until the next
    // edit; nothing below changes the text.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(text) = std::str::from_utf8(bytes) else { return };
    let runs = mdstyle::style_runs(text);
    e.call(SCI_SETINDICATORCURRENT, INDIC_STRIKE_ID, 0);
    e.call(SCI_INDICATORCLEARRANGE, 0, len as isize);
    e.call(SCI_STARTSTYLING, 0, 0);
    for (n, style) in segments(len, &runs) {
        e.call(SCI_SETSTYLING, n, style as isize);
    }
    for (range, style) in &runs {
        if *style == Style::Strike {
            e.call(SCI_INDICATORFILLRANGE, range.start, (range.end - range.start) as isize);
        }
    }
}

/// Back to plain text over the whole shown document.
pub fn clear(e: &Editor) {
    let len = e.text_len();
    e.call(SCI_SETINDICATORCURRENT, INDIC_STRIKE_ID, 0);
    e.call(SCI_INDICATORCLEARRANGE, 0, len as isize);
    e.call(SCI_STARTSTYLING, 0, 0);
    e.call(SCI_SETSTYLING, len, S_PLAIN as isize);
}

/// Carry out a `Plan` on the shown document.
pub fn run(e: &Editor, plan: Plan) {
    match plan {
        Plan::Nothing => {}
        Plan::Apply => apply(e),
        Plan::Clear => clear(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_has_its_own_number_below_32() {
        let all = [
            Style::Plain,
            Style::Heading(1),
            Style::Heading(2),
            Style::Heading(3),
            Style::Heading(4),
            Style::Heading(5),
            Style::Heading(6),
            Style::Bold,
            Style::Italic,
            Style::BoldItalic,
            Style::Strike,
            Style::InlineCode,
            Style::CodeBlock,
            Style::Link,
            Style::Url,
            Style::Quote,
            Style::TaskDone,
            Style::Marker,
        ];
        let numbers: Vec<u8> = all.iter().map(|s| style_number(*s)).collect();
        for (i, n) in numbers.iter().enumerate() {
            assert!(*n < 32, "{:?} -> {n}", all[i]);
            assert!(numbers[i + 1..].iter().all(|m| m != n), "{:?} shares {n}", all[i]);
        }
    }

    #[test]
    fn heading_levels_map_in_order_and_clamp() {
        assert_eq!(style_number(Style::Heading(1)), 2);
        assert_eq!(style_number(Style::Heading(6)), 7);
        assert_eq!(style_number(Style::Heading(9)), 7);
        assert_eq!(style_number(Style::Heading(0)), 2);
    }

    #[test]
    fn segments_fill_gaps_with_plain() {
        let runs = vec![(2..4, Style::Bold), (4..5, Style::Marker), (8..9, Style::Link)];
        assert_eq!(
            segments(10, &runs),
            vec![(2, S_PLAIN), (2, S_BOLD), (1, S_MARKER), (3, S_PLAIN), (1, S_LINK), (1, S_PLAIN)]
        );
    }

    #[test]
    fn segments_always_cover_the_whole_text() {
        for text in ["", "plain", "# Title\n\n**b** _i_ `c` [l](http://x.y)\n> q\n- [x] t\n```\ncode\n```\n"] {
            let runs = mdstyle::style_runs(text);
            let total: usize = segments(text.len(), &runs).iter().map(|(n, _)| n).sum();
            assert_eq!(total, text.len(), "{text:?}");
        }
    }

    #[test]
    fn segments_of_empty_text_are_empty() {
        assert!(segments(0, &[]).is_empty());
    }

    #[test]
    fn plan_applies_once_then_only_after_an_edit() {
        let mut a = Applied::<u32>::default();
        assert_eq!(a.plan(1, true), Plan::Apply);
        assert_eq!(a.plan(1, true), Plan::Nothing);
        a.touch(1);
        assert_eq!(a.plan(1, true), Plan::Apply);
        assert_eq!(a.plan(1, true), Plan::Nothing);
    }

    #[test]
    fn an_edit_in_a_tab_that_is_not_shown_is_restyled_when_it_is() {
        let mut a = Applied::<u32>::default();
        a.plan(1, true);
        a.plan(2, true);
        a.touch(1);
        // The timer fires with Tab 2 shown: Tab 1 stays stale.
        assert_eq!(a.plan(2, true), Plan::Nothing);
        assert_eq!(a.plan(1, true), Plan::Apply);
    }

    #[test]
    fn plan_clears_once_when_the_tab_stops_being_styled() {
        let mut a = Applied::<u32>::default();
        assert_eq!(a.plan(1, false), Plan::Nothing);
        a.plan(1, true);
        assert_eq!(a.plan(1, false), Plan::Clear);
        assert_eq!(a.plan(1, false), Plan::Nothing);
    }

    #[test]
    fn a_new_document_is_styled_again() {
        let mut a = Applied::<u32>::default();
        a.plan(1, true);
        a.forget(1);
        assert_eq!(a.plan(1, true), Plan::Apply);
        a.forget(1);
        assert_eq!(a.plan(1, false), Plan::Nothing);
    }

    /// One restyle pass over a ~900 KB markdown text in a real (never shown)
    /// Scintilla control. Run: `cargo test --release restyle_900kb -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn restyle_900kb() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::HWND_MESSAGE;
        let hinstance = unsafe { GetModuleHandleW(None).unwrap() }.into();
        assert!(Editor::register_class(hinstance));
        let parent: HWND = HWND_MESSAGE;
        let e = Editor::create(parent, hinstance, 96).expect("editor");
        let mut text = String::new();
        let para = "# Heading *one*

Some **bold** and _italic_ prose with `code`, a [link](http://example.com/x) and https://example.org/bare.

> quoted text

- [x] done
- [ ] todo

```rust
let x = 1;
```

";
        while text.len() < 900_000 {
            text.push_str(para);
        }
        e.fill(&text);
        let t = std::time::Instant::now();
        let runs = mdstyle::style_runs(&text);
        let parse = t.elapsed();
        let t = std::time::Instant::now();
        apply(&e);
        let total = t.elapsed();
        eprintln!("900 KB: {} runs; parse {parse:?}; full pass (parse + apply) {total:?}", runs.len());
    }
}
