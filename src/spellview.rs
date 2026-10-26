//! Spell check in the Source view (spec §5): squiggles under flagged words in
//! the visible lines plus a margin, and the right-click menu with suggestions.

use std::cell::{Cell, OnceCell};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, TrackPopupMenu, MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

use crate::editor::sci::*;
use crate::editor::{rgb, Editor};
use crate::mdstyle;
use crate::spell::{self, Checkers};

/// The indicator under flagged words (`mdview` uses 8).
const INDIC_SPELL: usize = 9;
const INDIC_SQUIGGLE: isize = 1;
/// Muted red.
const SQUIGGLE_INK: u32 = 0xC0554B;
/// Lines checked above and below the visible ones.
const MARGIN_LINES: isize = 40;
/// How many suggestions the menu offers.
const MAX_SUGGESTIONS: usize = 5;

const SCI_SETSEL: u32 = 2160;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_GETREADONLY: u32 = 2140;
const SCI_CANPASTE: u32 = 2173;
const SCI_CANUNDO: u32 = 2174;
const SCI_UNDO: u32 = 2176;
const SCI_CUT: u32 = 2177;
const SCI_COPY: u32 = 2178;
const SCI_PASTE: u32 = 2179;
const SCI_SELECTALL: u32 = 2013;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_POINTXFROMPOSITION: u32 = 2164;
const SCI_POINTYFROMPOSITION: u32 = 2165;
const SCI_CHARPOSITIONFROMPOINTCLOSE: u32 = 2562;
const SCI_LINESONSCREEN: u32 = 2370;
const SCI_USEPOPUP: u32 = 2371;
const SCI_INDICATORVALUEAT: u32 = 2507;
const SCI_INDICATORSTART: u32 = 2508;
const SCI_INDICATOREND: u32 = 2509;
const SCI_SETTARGETRANGE: u32 = 2686;
const SC_POPUP_NEVER: usize = 0;

// Menu command ids; suggestions follow from `ID_SUGGESTION`.
const ID_ADD: usize = 1;
const ID_UNDO: usize = 2;
const ID_CUT: usize = 3;
const ID_COPY: usize = 4;
const ID_PASTE: usize = 5;
const ID_SELECT_ALL: usize = 6;
const ID_SUGGESTION: usize = 100;

/// Define the indicator and turn Scintilla's own popup off. Call after the
/// editor's own look.
pub fn define(e: &Editor) {
    e.call(SCI_INDICSETSTYLE, INDIC_SPELL, INDIC_SQUIGGLE);
    e.call(SCI_INDICSETFORE, INDIC_SPELL, rgb(SQUIGGLE_INK));
    e.call(SCI_USEPOPUP, SC_POPUP_NEVER, 0);
}

pub struct SpellView {
    /// Opened on first use: creating them takes long enough to be felt at startup.
    checkers: OnceCell<Checkers>,
    /// The document lines (first, last) the shown document was last checked
    /// over, or `None` if it isn't spell checked.
    checked: Cell<Option<(isize, isize)>>,
}

impl SpellView {
    /// Opens Italian and English; either alone if the other isn't installed.
    pub fn new() -> SpellView {
        SpellView { checkers: OnceCell::new(), checked: Cell::new(None) }
    }

    fn checkers(&self) -> &Checkers {
        self.checkers.get_or_init(|| Checkers::open(&spell::LANGUAGES))
    }

    /// Check the shown document's visible lines (`enabled` is
    /// `Tab::spell_checked`), or clear the squiggles if it isn't spell checked.
    pub fn run(&self, e: &Editor, enabled: bool) {
        e.call(SCI_SETINDICATORCURRENT, INDIC_SPELL, 0);
        if !enabled || self.checkers().is_empty() {
            self.checked.set(None);
            e.call(SCI_INDICATORCLEARRANGE, 0, e.text_len() as isize);
            return;
        }
        let len = e.text_len();
        let ptr = e.call(SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: Scintilla's contiguous text, `len` bytes, valid until the next
        // edit; nothing below changes the text.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        let Ok(text) = std::str::from_utf8(bytes) else { return };
        let (first, last) = visible_lines(e);
        let from = (first - MARGIN_LINES).max(0);
        let to = (last + MARGIN_LINES).min(e.call(SCI_GETLINECOUNT, 0, 0) - 1);
        let start = e.call(SCI_POSITIONFROMLINE, from as usize, 0).max(0) as usize;
        let end = match e.call(SCI_POSITIONFROMLINE, (to + 1) as usize, 0) {
            p if p < 0 => len,
            p => p as usize,
        };
        let runs = mdstyle::style_runs(text);
        e.call(SCI_INDICATORCLEARRANGE, start, (end - start) as isize);
        for word in spell::words(text, &runs, start..end) {
            if self.checkers().is_misspelled(&text[word.clone()]) {
                e.call(SCI_INDICATORFILLRANGE, word.start, (word.end - word.start) as isize);
            }
        }
        self.checked.set(Some((from, to)));
    }

    /// The view has scrolled past the lines last checked.
    pub fn scrolled_away(&self, e: &Editor) -> bool {
        let Some((from, to)) = self.checked.get() else { return false };
        let (first, last) = visible_lines(e);
        first < from || last > to
    }

    /// The right-click menu at `lp` (`WM_CONTEXTMENU`: screen coordinates, or
    /// -1 from the keyboard). Holds no borrow of the app while the menu runs.
    pub fn context_menu(&self, e: &Editor, parent: HWND, lp: LPARAM, enabled: bool) {
        let (screen, pos) = locate(e, lp);
        let flagged = if enabled { self.flagged_word_at(e, pos) } else { None };
        // Right-click selects a flagged word; elsewhere it moves the caret
        // unless it lands inside the selection.
        match &flagged {
            Some((start, end, _)) => {
                e.call(SCI_SETSEL, *start, *end as isize);
            }
            None if pos >= 0 => {
                let (s, t) = (e.call(SCI_GETSELECTIONSTART, 0, 0), e.call(SCI_GETSELECTIONEND, 0, 0));
                if pos < s || pos > t {
                    e.call(SCI_GOTOPOS, pos as usize, 0);
                }
            }
            None => {}
        }
        let suggestions = flagged.as_ref().map(|(_, _, w)| self.checkers().suggest(w, MAX_SUGGESTIONS)).unwrap_or_default();
        let read_only = e.call(SCI_GETREADONLY, 0, 0) != 0;
        let has_selection = e.call(SCI_GETSELECTIONSTART, 0, 0) != e.call(SCI_GETSELECTIONEND, 0, 0);

        let choice = unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let add = |id: usize, text: &str, on: bool| {
                let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let state = if on { MF_STRING } else { MF_STRING | MF_GRAYED | MF_DISABLED };
                let _ = AppendMenuW(menu, state, id, PCWSTR(wide.as_ptr()));
            };
            let separator = || {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            };
            if flagged.is_some() {
                if suggestions.is_empty() {
                    add(0, "(No suggestions)", false);
                }
                for (i, s) in suggestions.iter().enumerate() {
                    add(ID_SUGGESTION + i, s, !read_only);
                }
                separator();
                add(ID_ADD, "Add to dictionary", true);
                separator();
            }
            add(ID_UNDO, "&Undo", !read_only && e.call(SCI_CANUNDO, 0, 0) != 0);
            separator();
            add(ID_CUT, "Cu&t", !read_only && has_selection);
            add(ID_COPY, "&Copy", has_selection);
            add(ID_PASTE, "&Paste", !read_only && e.call(SCI_CANPASTE, 0, 0) != 0);
            separator();
            add(ID_SELECT_ALL, "Select &all", true);
            let id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, screen.x, screen.y, None, parent, None).0;
            let _ = DestroyMenu(menu);
            id as usize
        };

        match choice {
            ID_ADD => {
                if let Some((_, _, word)) = &flagged {
                    self.checkers().add(word);
                    self.run(e, true);
                }
            }
            ID_UNDO => {
                e.call(SCI_UNDO, 0, 0);
            }
            ID_CUT => {
                e.call(SCI_CUT, 0, 0);
            }
            ID_COPY => {
                e.call(SCI_COPY, 0, 0);
            }
            ID_PASTE => {
                e.call(SCI_PASTE, 0, 0);
            }
            ID_SELECT_ALL => {
                e.call(SCI_SELECTALL, 0, 0);
            }
            n if n >= ID_SUGGESTION => {
                // The edit marks the Tab unsaved and re-checks after the pause.
                if let (Some((start, end, _)), Some(s)) = (&flagged, suggestions.get(n - ID_SUGGESTION)) {
                    e.call(SCI_SETTARGETRANGE, *start, *end as isize);
                    e.call(SCI_REPLACETARGET, s.len(), s.as_ptr() as isize);
                }
            }
            _ => {}
        }
    }

    /// The flagged word under `pos`: its byte range and text.
    fn flagged_word_at(&self, e: &Editor, pos: isize) -> Option<(usize, usize, String)> {
        if pos < 0 || e.call(SCI_INDICATORVALUEAT, INDIC_SPELL, pos) == 0 {
            return None;
        }
        let start = e.call(SCI_INDICATORSTART, INDIC_SPELL, pos) as usize;
        let end = e.call(SCI_INDICATOREND, INDIC_SPELL, pos) as usize;
        let len = e.text_len();
        let ptr = e.call(SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
        if ptr.is_null() || end > len || start >= end {
            return None;
        }
        // SAFETY: as in `run`.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.add(start), end - start) };
        Some((start, end, std::str::from_utf8(bytes).ok()?.to_string()))
    }
}

/// First and last document lines in view.
fn visible_lines(e: &Editor) -> (isize, isize) {
    let top = e.call(SCI_GETFIRSTVISIBLELINE, 0, 0);
    let on_screen = e.call(SCI_LINESONSCREEN, 0, 0);
    let first = e.call(SCI_DOCLINEFROMVISIBLE, top as usize, 0);
    let last = e.call(SCI_DOCLINEFROMVISIBLE, (top + on_screen) as usize, 0);
    (first, last)
}

/// Where a `WM_CONTEXTMENU` points: the screen point, and the document
/// position of the character there (-1 if none).
fn locate(e: &Editor, lp: LPARAM) -> (POINT, isize) {
    let (x, y) = ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32);
    if lp.0 as i32 == -1 {
        // From the keyboard: at the caret.
        let caret = e.call(SCI_GETCURRENTPOS, 0, 0);
        let mut pt = POINT {
            x: e.call(SCI_POINTXFROMPOSITION, 0, caret) as i32,
            y: e.call(SCI_POINTYFROMPOSITION, 0, caret) as i32 + 16,
        };
        unsafe {
            let _ = ClientToScreen(e.hwnd(), &mut pt);
        }
        return (pt, caret);
    }
    let mut client = POINT { x, y };
    unsafe {
        let _ = ScreenToClient(e.hwnd(), &mut client);
    }
    let pos = e.call(SCI_CHARPOSITIONFROMPOINTCLOSE, client.x as usize, client.y as isize);
    (POINT { x, y }, pos)
}
