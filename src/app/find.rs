//! The find bar (spec §4, §11): the Win32 EDIT controls in its band, and what
//! Ctrl+F, Ctrl+H and Ctrl+G do. The search rules are `crate::find`; the
//! layout, hit test and drawing are `chrome::findbar`. Wrap and zoom live in
//! `findview`.
//!
//! The bar's state is thread-local, like the menu bar's. Never hold it across
//! a Win32 call: setting an EDIT's text sends `EN_CHANGE` straight back.

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, HFONT, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_SETCUEBANNER, EM_SETSEL};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_F3, VK_RETURN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{app, invalidate, relayout, scale_of, App};
use crate::chrome::findbar::{self, Field, FindHit, FindLayout, Mode};
use crate::chrome::FindView;
use crate::editor::MATCH_INDICATOR;
use crate::find::{self, Matches};
use crate::proto::{FIND_MS, TIMER_FIND};
use crate::tabs::Tab;

/// The EDIT controls' child ids; `WM_COMMAND` carries them with a notification.
const ID_FIND: u16 = 0x1A1;
const ID_REPLACE: u16 = 0x1A2;
const ID_GOTO: u16 = 0x1A3;
const EN_SETFOCUS: u16 = 0x0100;
const EN_KILLFOCUS: u16 = 0x0200;
const EN_CHANGE: u16 = 0x0300;
/// A selection longer than this is not taken as the word to find.
const PREFILL_MAX: usize = 200;
const SUBCLASS_ID: usize = 1;
/// Input text height, in DIPs.
const EDIT_FONT: f32 = 13.0;

struct FindUi {
    mode: Mode,
    match_case: bool,
    /// Find, Replace and Go to line's EDITs, made on first use.
    edits: Option<[HWND; 3]>,
    font: Option<(u32, HFONT)>,
    matches: Matches,
    /// The find field is empty: no count.
    no_needle: bool,
    current: Option<usize>,
    /// What the count says instead of "n of m" until the next search ("3 replaced").
    count_note: Option<String>,
    hover: Option<FindHit>,
    focus: Option<Field>,
    /// An EN_CHANGE caused by our own `SetWindowTextW`.
    quiet: bool,
}

thread_local! {
    static FIND: RefCell<FindUi> = const { RefCell::new(FindUi {
        mode: Mode::Hidden,
        match_case: false,
        edits: None,
        font: None,
        matches: Matches { list: Vec::new(), capped: false },
        no_needle: true,
        current: None,
        count_note: None,
        hover: None,
        focus: None,
        quiet: false,
    }) };
}

fn st<R>(f: impl FnOnce(&mut FindUi) -> R) -> R {
    FIND.with(|s| f(&mut s.borrow_mut()))
}

fn field_of(id: u16) -> Option<Field> {
    match id {
        ID_FIND => Some(Field::Find),
        ID_REPLACE => Some(Field::Replace),
        ID_GOTO => Some(Field::Goto),
        _ => None,
    }
}

fn edit(field: Field) -> Option<HWND> {
    st(|f| f.edits).map(|e| e[field as usize])
}

/// The bar's height in DIPs, for `Frame::find_h`: 0 when hidden.
pub fn band_height() -> f32 {
    findbar::height(st(|f| f.mode))
}

fn open_mode() -> Mode {
    st(|f| f.mode)
}

fn layout(a: &App, mode: Mode) -> FindLayout {
    let band = a.ui.borrow().layout.find;
    FindLayout::new(band, scale_of(a.hwnd), mode)
}

fn shown_read_only(a: &App) -> bool {
    let tabs = a.tabs.borrow();
    tabs.active().and_then(|id| tabs.get(id)).is_some_and(Tab::read_only)
}

fn invalidate_band(a: &App) {
    let band = a.ui.borrow().layout.find;
    invalidate(a, Some(band));
}

// --- The EDIT controls ---------------------------------------------------------

fn text(field: Field) -> String {
    let Some(hwnd) = edit(field) else { return String::new() };
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let got = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got])
    }
}

/// Put `s` in an input without it counting as typing.
fn set_text(field: Field, s: &str) {
    let Some(hwnd) = edit(field) else { return };
    st(|f| f.quiet = true);
    let z: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(z.as_ptr()));
    }
    st(|f| f.quiet = false);
}

fn focus_field(field: Field) {
    let Some(hwnd) = edit(field) else { return };
    unsafe {
        let _ = SetFocus(Some(hwnd));
        SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    }
}

fn ensure_controls(a: &App) {
    if st(|f| f.edits.is_some()) {
        return;
    }
    let hinstance = unsafe { GetModuleHandleW(None) }.map(Into::into).ok();
    let make = |id: u16, extra: WINDOW_STYLE, cue: PCWSTR| unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32) | extra,
            0,
            0,
            0,
            0,
            Some(a.hwnd),
            Some(HMENU(id as usize as *mut _)),
            hinstance,
            None,
        )
        .unwrap_or_default();
        SendMessageW(hwnd, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.0 as isize)));
        let _ = SetWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID, 0);
        hwnd
    };
    let edits = [
        make(ID_FIND, WINDOW_STYLE(0), w!("Find")),
        make(ID_REPLACE, WINDOW_STYLE(0), w!("Replace with")),
        make(ID_GOTO, WINDOW_STYLE(ES_NUMBER as u32), w!("Line number")),
    ];
    st(|f| f.edits = Some(edits));
}

/// Move the controls into the band, and show the ones the mode uses. Called
/// by `relayout`, so a resize or a DPI change keeps them in place.
pub fn place(a: &App) {
    let Some(edits) = st(|f| f.edits) else { return };
    let mode = open_mode();
    let l = layout(a, mode);
    let dpi = unsafe { GetDpiForWindow(a.hwnd) };
    let font = st(|f| f.font.filter(|(d, _)| *d == dpi));
    let font = font.map(|(_, h)| h).unwrap_or_else(|| {
        let px = (EDIT_FONT * dpi as f32 / 96.0).round() as i32;
        let h = unsafe {
            CreateFontW(-px, 0, 0, 0, 400, 0, 0, 0, FONT_CHARSET(1), FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5), 0, w!("Segoe UI"))
        };
        let old = st(|f| f.font.replace((dpi, h)));
        for e in edits {
            unsafe { SendMessageW(e, WM_SETFONT, Some(WPARAM(h.0 as usize)), Some(LPARAM(1))) };
        }
        if let Some((_, old)) = old {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(old.0));
            }
        }
        h
    });
    let _ = font;
    for (field, hwnd) in [(Field::Find, edits[0]), (Field::Replace, edits[1]), (Field::Goto, edits[2])] {
        let shown = match mode {
            Mode::Hidden => false,
            Mode::Find => field == Field::Find,
            Mode::Replace => field != Field::Goto,
            Mode::Goto => field == Field::Goto,
        };
        unsafe {
            if shown {
                let r = l.edit_rect(field);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            } else {
                let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_HIDEWINDOW);
            }
        }
    }
}

/// Keys in an input: Enter, Shift+Enter, Esc, Tab and F3 are the bar's.
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    unsafe {
        let field = field_of(GetDlgCtrlID(hwnd) as u16);
        if let (Some(field), Some(a)) = (field, app()) {
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
            match msg {
                WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                    VK_RETURN => {
                        enter(a, field, shift);
                        return LRESULT(0);
                    }
                    VK_ESCAPE => {
                        close(a);
                        return LRESULT(0);
                    }
                    VK_F3 if field != Field::Goto => {
                        step(a, !shift);
                        return LRESULT(0);
                    }
                    VK_TAB => {
                        tab_to(field);
                        return LRESULT(0);
                    }
                    // Ctrl+A: a one-line EDIT does not select all by itself.
                    k if k.0 == b'A' as u16 && GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                        SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                        return LRESULT(0);
                    }
                    _ => {}
                },
                // No beep for the keys above.
                WM_CHAR if matches!(wp.0, 0x0D | 0x1B | 0x09 | 0x01) => return LRESULT(0),
                _ => {}
            }
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

/// `WM_COMMAND` from an input. Returns false if it is not one of ours.
pub fn control_notify(a: &App, wp: WPARAM, lp: LPARAM) -> bool {
    let (id, code) = ((wp.0 & 0xffff) as u16, ((wp.0 >> 16) & 0xffff) as u16);
    let Some(field) = field_of(id).filter(|_| lp.0 != 0) else { return false };
    match code {
        EN_SETFOCUS => {
            st(|f| f.focus = Some(field));
            invalidate_band(a);
        }
        EN_KILLFOCUS => {
            st(|f| f.focus = None);
            invalidate_band(a);
        }
        EN_CHANGE if !st(|f| f.quiet) && field == Field::Find => typed(a),
        _ => {}
    }
    true
}

// --- Opening and closing -------------------------------------------------------

/// Ctrl+F, Ctrl+H, Ctrl+G: open the bar in `mode` with the keyboard in its
/// first input. Find and Replace take the selection as the word to find.
pub fn open(a: &App, mode: Mode) {
    // A read-only Tab can be searched, not replaced in.
    let mode = if mode == Mode::Replace && shown_read_only(a) { Mode::Find } else { mode };
    ensure_controls(a);
    a.editor.define_find_indicators();
    let word = (mode != Mode::Goto).then(|| a.editor.selected_word(PREFILL_MAX)).flatten();
    let before = st(|f| std::mem::replace(&mut f.mode, mode));
    if before != mode {
        relayout(a);
    }
    let first = if mode == Mode::Goto { Field::Goto } else { Field::Find };
    if let Some(word) = word {
        set_text(Field::Find, &word);
    }
    if mode == Mode::Goto {
        a.editor.clear_find_marks();
        st(|f| f.matches = Matches::default());
    } else {
        refresh(a);
    }
    focus_field(first);
    invalidate_band(a);
}

/// Esc, ×: hide the bar, drop the highlights, and give the keyboard back to the text.
pub fn close(a: &App) {
    if st(|f| std::mem::replace(&mut f.mode, Mode::Hidden)) == Mode::Hidden {
        return;
    }
    st(|f| {
        f.matches = Matches::default();
        f.current = None;
        f.count_note = None;
        f.hover = None;
        f.focus = None;
    });
    a.editor.clear_find_marks();
    unsafe {
        let _ = SetFocus(Some(a.editor.hwnd()));
    }
    relayout(a);
}

/// Tab → Tab: the document changed under the bar.
pub fn on_show(a: &App) {
    a.editor.clear_find_marks();
    // The new Tab can't be replaced in: the replace row goes.
    if open_mode() == Mode::Replace && shown_read_only(a) {
        st(|f| f.mode = Mode::Find);
        relayout(a);
    }
    if matches!(open_mode(), Mode::Find | Mode::Replace) {
        refresh(a);
    }
}

/// The text was edited: find the matches again once typing pauses.
pub fn edited(a: &App) {
    if matches!(open_mode(), Mode::Find | Mode::Replace) {
        unsafe {
            SetTimer(Some(a.hwnd), TIMER_FIND, FIND_MS, None);
        }
    }
}

pub fn timer(a: &App) {
    if matches!(open_mode(), Mode::Find | Mode::Replace) {
        refresh(a);
    }
}

// --- Searching -------------------------------------------------------------------

fn needle() -> String {
    text(Field::Find)
}

fn match_case() -> bool {
    st(|f| f.match_case)
}

/// Find every match again: highlights, "n of m".
fn refresh(a: &App) {
    let (needle, case) = (needle(), match_case());
    let matches = find::all_matches(&a.editor, &needle, case);
    a.editor.clear_find_marks();
    a.editor.mark_ranges(MATCH_INDICATOR, &matches.list);
    st(|f| {
        f.matches = matches;
        f.no_needle = needle.is_empty();
        f.count_note = None;
    });
    mark_current(a);
}

/// Which match is selected: the strong highlight and the count.
fn mark_current(a: &App) {
    let sel = a.editor.selection();
    let current = st(|f| f.matches.index_of(&sel));
    let mut range = current.and_then(|i| st(|f| f.matches.list.get(i).cloned()));
    // Past the list's cap the match is still the current one, just not counted.
    if range.is_none() && st(|f| f.matches.capped) {
        let (needle, case) = (needle(), match_case());
        range = find::next_match(&a.editor, &needle, case, sel.start).filter(|m| *m == sel);
    }
    a.editor.set_current_mark(range.as_ref());
    st(|f| f.current = current);
    invalidate_band(a);
}

/// Typing in the find field: show the matches, and select the first one from
/// where the selection starts, without taking the keyboard from the field.
fn typed(a: &App) {
    refresh(a);
    let (needle, case) = (needle(), match_case());
    let from = a.editor.selection().start;
    if let Some(m) = find::next_match(&a.editor, &needle, case, from) {
        a.editor.select(&m);
        mark_current(a);
    }
}

/// Enter (and Shift+Enter) in an input.
fn enter(a: &App, field: Field, shift: bool) {
    match field {
        Field::Find => step(a, !shift),
        Field::Replace => replace_one(a),
        Field::Goto => go_to(a),
    }
}

/// Tab and Shift+Tab move between the find and replace fields.
fn tab_to(from: Field) {
    let to = match (open_mode(), from) {
        (Mode::Replace, Field::Find) => Field::Replace,
        (Mode::Replace, Field::Replace) => Field::Find,
        _ => from,
    };
    focus_field(to);
}

/// ↓ / Enter (`forward`), ↑ / Shift+Enter: select the next or previous match.
pub fn step(a: &App, forward: bool) {
    let (needle, case) = (needle(), match_case());
    let sel = a.editor.selection();
    let found = if forward {
        find::next_match(&a.editor, &needle, case, sel.end)
    } else {
        find::prev_match(&a.editor, &needle, case, sel.start)
    };
    if let Some(m) = found {
        a.editor.select(&m);
    }
    mark_current(a);
}

fn replace_one(a: &App) {
    if shown_read_only(a) {
        return;
    }
    let (needle, with, case) = (needle(), text(Field::Replace), match_case());
    let mut e = a.editor;
    let sel = e.selection();
    match find::replace_selected(&mut e, &needle, &with, case, sel) {
        Some(after) => {
            refresh(a);
            match find::next_match(&a.editor, &needle, case, after) {
                Some(m) => a.editor.select(&m),
                None => a.editor.select(&(after..after)),
            }
            mark_current(a);
        }
        // Nothing selected to replace: go to the next match first.
        None => step(a, true),
    }
}

fn replace_all(a: &App) {
    if shown_read_only(a) {
        return;
    }
    let (needle, with, case) = (needle(), text(Field::Replace), match_case());
    let mut e = a.editor;
    let n = find::replace_all(&mut e, &needle, &with, case);
    refresh(a);
    if n > 0 {
        st(|f| f.count_note = Some(format!("{n} replaced")));
        invalidate_band(a);
    }
}

fn go_to(a: &App) {
    let lines = a.editor.call(crate::editor::sci::SCI_GETLINECOUNT, 0, 0).max(1) as usize;
    if let Some(line) = find::go_to_line(&text(Field::Goto), lines) {
        close(a);
        a.editor.go_to_line(line);
    }
}

// --- Mouse ---------------------------------------------------------------------

/// A click in the band.
pub fn click(a: &App, x: i32, y: i32) {
    let mode = open_mode();
    match layout(a, mode).hit_test(x, y) {
        FindHit::Field(f) => focus_field(f),
        FindHit::MatchCase => {
            st(|f| f.match_case = !f.match_case);
            typed(a);
        }
        FindHit::Prev => step(a, false),
        FindHit::Next => step(a, true),
        FindHit::Close => close(a),
        FindHit::Replace => replace_one(a),
        FindHit::ReplaceAll => replace_all(a),
        FindHit::None => {}
    }
}

pub fn hover(a: &App, x: i32, y: i32) {
    let mode = open_mode();
    let hit = Some(layout(a, mode).hit_test(x, y)).filter(|h| *h != FindHit::None && !matches!(h, FindHit::Field(_)));
    if st(|f| std::mem::replace(&mut f.hover, hit)) != hit {
        invalidate_band(a);
    }
}

pub fn leave(a: &App) {
    if st(|f| f.hover.take()).is_some() {
        invalidate_band(a);
    }
}

/// What a paint shows, or `None` while the bar is hidden.
pub fn view(a: &App) -> Option<FindView> {
    let mode = open_mode();
    if mode == Mode::Hidden {
        return None;
    }
    let l = layout(a, mode);
    Some(st(|f| FindView {
        layout: l,
        match_case: f.match_case,
        count: f.count_note.clone().unwrap_or_else(|| find::count_label(f.no_needle, f.current, &f.matches)),
        hover: f.hover,
        focus: f.focus,
    }))
}
