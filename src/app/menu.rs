//! File, Edit and View (spec §4): what each popup holds, as data, and how
//! the keyboard moves across the menu buttons like a native menu bar.
//!
//! Each popup is a list of [`Entry`]s: 12 (find, wrap, zoom) and 13 (the
//! Rendered view) enable theirs by changing the `enabled` rule. [`items`]
//! turns a list into what one popup shows for the active Tab.

use std::cell::RefCell;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_F10, VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{app, cmd, command, files, invalidate, wide, App};
use crate::chrome::Hit;
use crate::editor::sci;
use crate::tabs::{Action, TabId, Tabs};
use crate::textfile::{Encoding, Eol, NEW_FILE};

/// What the popups depend on: the active Tab and the editor.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub encoding: Encoding,
    pub eol: Eol,
    pub read_only: bool,
    pub untitled: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Some text is selected.
    pub selection: bool,
    pub can_paste: bool,
}

/// One line of a popup, ready to show.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// `label` holds the `&` access key and, after a tab, the shortcut.
    Command { label: String, id: u16, enabled: bool, checked: bool },
    Sub { label: String, enabled: bool, items: Vec<Item> },
    Separator,
}

/// One line of a popup's definition.
pub enum Entry {
    Command { label: &'static str, keys: &'static str, id: u16, enabled: fn(&State) -> bool },
    Sub { label: &'static str, enabled: fn(&State) -> bool, items: fn(&State) -> Vec<Item> },
    Separator,
}

fn always(_: &State) -> bool {
    true
}

/// Waits for a later ticket (12, 13): shown greyed.
fn later(_: &State) -> bool {
    false
}

fn writable(s: &State) -> bool {
    !s.read_only
}

pub const FILE: &[Entry] = &[
    Entry::Command { label: "&New tab", keys: "Ctrl+T", id: cmd::NEW_TAB, enabled: always },
    Entry::Command { label: "&Open\u{2026}", keys: "Ctrl+O", id: cmd::OPEN, enabled: always },
    // 15 fills it.
    Entry::Sub { label: "Recent &files", enabled: later, items: |_| Vec::new() },
    Entry::Separator,
    Entry::Command { label: "&Save", keys: "Ctrl+S", id: cmd::SAVE, enabled: always },
    Entry::Command { label: "Save &as\u{2026}", keys: "Ctrl+Shift+S", id: cmd::SAVE_AS, enabled: always },
    Entry::Separator,
    Entry::Sub { label: "&Encoding", enabled: always, items: encodings },
    Entry::Sub { label: "&Line endings", enabled: always, items: line_endings },
    Entry::Separator,
    Entry::Command { label: "&Close tab", keys: "Ctrl+W", id: cmd::CLOSE_TAB, enabled: always },
    Entry::Command { label: "E&xit", keys: "", id: cmd::EXIT, enabled: always },
];

pub const EDIT: &[Entry] = &[
    Entry::Command { label: "&Undo", keys: "Ctrl+Z", id: cmd::UNDO, enabled: |s| s.can_undo },
    Entry::Command { label: "&Redo", keys: "Ctrl+Y", id: cmd::REDO, enabled: |s| s.can_redo },
    Entry::Separator,
    Entry::Command { label: "Cu&t", keys: "Ctrl+X", id: cmd::CUT, enabled: |s| s.selection && !s.read_only },
    Entry::Command { label: "&Copy", keys: "Ctrl+C", id: cmd::COPY, enabled: |s| s.selection },
    Entry::Command { label: "&Paste", keys: "Ctrl+V", id: cmd::PASTE, enabled: |s| s.can_paste && !s.read_only },
    Entry::Command { label: "Select &all", keys: "Ctrl+A", id: cmd::SELECT_ALL, enabled: always },
    Entry::Separator,
    Entry::Command { label: "&Find\u{2026}", keys: "Ctrl+F", id: cmd::FIND, enabled: later },
    Entry::Command { label: "R&eplace\u{2026}", keys: "Ctrl+H", id: cmd::REPLACE, enabled: later },
    Entry::Command { label: "&Go to line\u{2026}", keys: "Ctrl+G", id: cmd::GO_TO_LINE, enabled: later },
];

pub const VIEW: &[Entry] = &[
    Entry::Command { label: "&Rendered view", keys: "Ctrl+E", id: cmd::RENDERED_VIEW, enabled: later },
    Entry::Command { label: "&Word wrap", keys: "Alt+Z", id: cmd::WORD_WRAP, enabled: later },
    Entry::Separator,
    Entry::Command { label: "Zoom &in", keys: "Ctrl++", id: cmd::ZOOM_IN, enabled: later },
    Entry::Command { label: "Zoom &out", keys: "Ctrl+\u{2212}", id: cmd::ZOOM_OUT, enabled: later },
    Entry::Command { label: "Reset &zoom", keys: "Ctrl+0", id: cmd::ZOOM_RESET, enabled: later },
];

/// File, Edit, View, in the order of their buttons.
pub const MENUS: [&[Entry]; 3] = [FILE, EDIT, VIEW];

/// The `&` labels of `Encoding::MENU`.
const ENCODING_LABELS: [&str; 5] = ["&UTF-8", "UTF-8 with &BOM", "UTF-16 &LE", "UTF-16 B&E", "&Windows-1252 (ANSI)"];
/// The `&` labels of `Eol::MENU`.
const EOL_LABELS: [&str; 3] = ["&LF", "C&RLF", "&CR"];

/// What a popup shows for `state`.
pub fn items(entries: &[Entry], state: &State) -> Vec<Item> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Command { label, keys, id, enabled } => Item::Command {
                label: if keys.is_empty() { label.to_string() } else { format!("{label}\t{keys}") },
                id: *id,
                enabled: enabled(state),
                checked: false,
            },
            Entry::Sub { label, enabled, items } => {
                Item::Sub { label: label.to_string(), enabled: enabled(state), items: items(state) }
            }
            Entry::Separator => Item::Separator,
        })
        .collect()
}

/// Encoding ›: the five choices, the Tab's own first if the menu lacks it,
/// then Reopen with encoding….
fn encodings(s: &State) -> Vec<Item> {
    let mut items = Vec::new();
    if !Encoding::MENU.contains(&s.encoding) {
        items.push(Item::Command { label: s.encoding.name().into(), id: cmd::ENCODING_KEEP, enabled: true, checked: true });
        items.push(Item::Separator);
    }
    for (i, (e, label)) in Encoding::MENU.iter().zip(ENCODING_LABELS).enumerate() {
        items.push(Item::Command {
            label: label.into(),
            id: cmd::ENCODING + i as u16,
            enabled: !s.read_only,
            checked: *e == s.encoding,
        });
    }
    items.push(Item::Separator);
    items.push(Item::Command {
        label: "&Reopen with encoding\u{2026}".into(),
        id: cmd::REOPEN,
        enabled: !s.untitled,
        checked: false,
    });
    items
}

/// Line endings ›: LF, CRLF, CR.
fn line_endings(s: &State) -> Vec<Item> {
    Eol::MENU
        .iter()
        .zip(EOL_LABELS)
        .enumerate()
        .map(|(i, (e, label))| Item::Command {
            label: label.into(),
            id: cmd::EOL + i as u16,
            enabled: writable(s),
            checked: *e == s.eol,
        })
        .collect()
}

// --- The menu bar from the keyboard ----------------------------------------

/// A key, as the menu bar sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Esc,
    Alt,
    F10,
    Letter(char),
    Other,
}

/// What a key does while a menu button is highlighted (Alt or F10 pressed
/// alone, no popup open).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    Focus(usize),
    Open(usize),
    Leave,
}

/// The menu whose access key is `c`: F, E or V.
pub fn access_key(c: char) -> Option<usize> {
    ['f', 'e', 'v'].iter().position(|k| *k == c.to_ascii_lowercase())
}

fn step(at: usize, by: isize) -> usize {
    (at as isize + by).rem_euclid(MENUS.len() as isize) as usize
}

/// A key while menu `at` is highlighted.
pub fn on_bar(at: usize, key: Key) -> Bar {
    match key {
        Key::Left => Bar::Focus(step(at, -1)),
        Key::Right => Bar::Focus(step(at, 1)),
        Key::Up | Key::Down | Key::Enter => Bar::Open(at),
        Key::Esc | Key::Alt | Key::F10 => Bar::Leave,
        Key::Letter(c) => access_key(c).map_or(Bar::Focus(at), Bar::Open),
        Key::Other => Bar::Focus(at),
    }
}

/// Where the selection is in an open popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Popup {
    /// In a submenu (Encoding ›, ...), not the popup itself.
    pub in_submenu: bool,
    /// On an item that opens a submenu.
    pub on_submenu_item: bool,
}

/// A key while popup `at` is open: the menu to move to, if any. Left and
/// Right move across the bar unless they open or close a submenu.
pub fn in_popup(at: usize, key: Key, popup: Popup) -> Option<usize> {
    match key {
        Key::Left if !popup.in_submenu => Some(step(at, -1)),
        Key::Right if !popup.on_submenu_item => Some(step(at, 1)),
        _ => None,
    }
}

// --- Win32 -------------------------------------------------------------------

/// Where the menu bar is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    /// Highlighted from the keyboard (Alt or F10 alone), no popup open.
    Focused(usize),
    /// A popup is open; `true` if the keyboard opened it.
    Open(usize, bool),
}

struct Shared {
    mode: Mode,
    /// Alt is held: the access keys show.
    alt_held: bool,
    /// The key whose release must not reach Scintilla: it left the bar.
    eat_up: Option<u16>,
}

/// While a popup is open: what the message hook needs.
struct Tracking {
    at: usize,
    top: HMENU,
    popup: Popup,
    /// Left, Right or the mouse moved to this menu (`true`: by keyboard).
    next: Option<(usize, bool)>,
    /// Esc closed the popup itself, not a submenu: back to the highlighted bar.
    escaped: bool,
}

thread_local! {
    static SHARED: RefCell<Shared> = const { RefCell::new(Shared { mode: Mode::Idle, alt_held: false, eat_up: None }) };
    static TRACKING: RefCell<Option<Tracking>> = const { RefCell::new(None) };
}

fn mode() -> Mode {
    SHARED.with(|s| s.borrow().mode)
}

/// Change the bar's state and repaint the title row if that shows.
fn update(a: &App, f: impl FnOnce(&mut Shared)) {
    let before = painted();
    SHARED.with(|s| f(&mut s.borrow_mut()));
    if painted() != before {
        let title = a.ui.borrow().layout.title;
        invalidate(a, Some(title));
    }
}

/// For the painter: the menu button drawn highlighted, and whether the
/// access keys (F, E, V) are underlined.
pub fn painted() -> (Option<usize>, bool) {
    let (mode, alt_held) = SHARED.with(|s| {
        let s = s.borrow();
        (s.mode, s.alt_held)
    });
    match mode {
        Mode::Idle => (None, alt_held || keyboard_cues()),
        Mode::Focused(at) => (Some(at), true),
        Mode::Open(at, keyboard) => (Some(at), keyboard || keyboard_cues()),
    }
}

/// "Underline access keys" is on in Settings.
fn keyboard_cues() -> bool {
    let mut on = BOOL(0);
    let got = unsafe {
        SystemParametersInfoW(SPI_GETKEYBOARDCUES, 0, Some(&mut on as *mut BOOL as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    got.is_ok() && on.as_bool()
}

fn key_of(vk: u16) -> Key {
    match VIRTUAL_KEY(vk) {
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Esc,
        VK_MENU => Key::Alt,
        VK_F10 => Key::F10,
        _ if (b'A' as u16..=b'Z' as u16).contains(&vk) || (b'0' as u16..=b'9' as u16).contains(&vk) => {
            Key::Letter(vk as u8 as char)
        }
        _ => Key::Other,
    }
}

/// Before translation, in the message loop: while the bar is highlighted it
/// takes every key; Alt held shows the access keys; a click leaves the bar.
/// Returns true for a message it ate.
pub fn pre_translate(msg: &MSG) -> bool {
    let Some(a) = app() else { return false };
    let vk = msg.wParam.0 as u16;
    match msg.message {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN => {
            if matches!(mode(), Mode::Focused(_)) {
                update(a, |s| s.mode = Mode::Idle);
            }
            false
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Mode::Focused(at) = mode() {
                let step = on_bar(at, key_of(vk));
                update(a, |s| {
                    s.mode = match step {
                        Bar::Focus(to) => Mode::Focused(to),
                        Bar::Open(_) | Bar::Leave => Mode::Idle,
                    };
                    if step == Bar::Leave {
                        s.eat_up = Some(vk);
                    }
                });
                if let Bar::Open(to) = step {
                    open(a, to, true);
                }
                return true;
            }
            // Bit 30: a repeat while held.
            if msg.message == WM_SYSKEYDOWN && VIRTUAL_KEY(vk) == VK_MENU && msg.lParam.0 & (1 << 30) == 0 {
                update(a, |s| s.alt_held = true);
            }
            false
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let eat = SHARED.with(|s| s.borrow().eat_up) == Some(vk);
            if VIRTUAL_KEY(vk) == VK_MENU || eat {
                update(a, |s| {
                    s.alt_held = false;
                    s.eat_up = None;
                });
            }
            eat || matches!(mode(), Mode::Focused(_))
        }
        WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR => matches!(mode(), Mode::Focused(_)),
        _ => false,
    }
}

/// `WM_SYSCOMMAND` / `SC_KEYMENU` from `DefWindowProc`: `ch` is 0 for Alt or
/// F10 pressed alone, else the letter pressed with Alt. Returns false for a
/// key that is not ours (Alt+Space opens the system menu).
pub fn key_menu(a: &App, ch: u32) -> bool {
    if ch == 0 {
        update(a, |s| s.mode = Mode::Focused(0));
        return true;
    }
    match char::from_u32(ch).and_then(access_key) {
        Some(at) => {
            open(a, at, true);
            true
        }
        None => false,
    }
}

/// The Window lost activation: the bar is no longer highlighted.
pub fn leave(a: &App) {
    update(a, |s| {
        s.mode = Mode::Idle;
        s.alt_held = false;
    });
}

/// Open popup `at` under its button, and follow Left, Right and the mouse
/// to the next one until a command is picked or the popups close. Pumps
/// messages (`TrackPopupMenuEx`), so nothing may be borrowed across it.
pub fn open(a: &App, at: usize, keyboard: bool) {
    let mut next = Some((at, keyboard));
    while let Some((at, keyboard)) = next.take() {
        update(a, |s| s.mode = Mode::Open(at, keyboard));
        let (chosen, tracking) = track(a, at, keyboard);
        next = tracking.as_ref().and_then(|t| t.next);
        if next.is_some() {
            continue;
        }
        let escaped = tracking.is_some_and(|t| t.escaped);
        update(a, |s| {
            s.mode = if escaped && chosen == 0 { Mode::Focused(at) } else { Mode::Idle };
            s.alt_held = false;
        });
        if chosen != 0 {
            unsafe {
                SendMessageW(a.hwnd, WM_COMMAND, Some(WPARAM(chosen as usize)), Some(LPARAM(0)));
            }
        }
    }
}

/// Show one popup; the command picked (0 for none) and how it closed.
fn track(a: &App, at: usize, keyboard: bool) -> (u16, Option<Tracking>) {
    let list = items(MENUS[at], &state_of(a));
    let Some(menu) = build(&list) else { return (0, None) };
    let button = a.ui.borrow().layout.menus[at];
    let mut corners = [POINT { x: button.left, y: button.top }, POINT { x: button.right, y: button.bottom }];
    unsafe {
        for p in &mut corners {
            let _ = ClientToScreen(a.hwnd, p);
        }
    }
    let params = TPMPARAMS {
        cbSize: std::mem::size_of::<TPMPARAMS>() as u32,
        // Never cover the button: flipped above it if there is no room below.
        rcExclude: RECT { left: corners[0].x, top: corners[0].y, right: corners[1].x, bottom: corners[1].y },
    };
    TRACKING.with(|t| {
        *t.borrow_mut() = Some(Tracking {
            at,
            top: menu,
            popup: Popup { in_submenu: false, on_submenu_item: false },
            next: None,
            escaped: false,
        })
    });
    let chosen = unsafe {
        // Opened from the keyboard, a native menu bar selects the first item.
        if keyboard {
            let _ = PostMessageW(Some(a.hwnd), WM_KEYDOWN, WPARAM(VK_DOWN.0 as usize), LPARAM(0));
        }
        let hook = SetWindowsHookExW(WH_MSGFILTER, Some(filter), None, GetCurrentThreadId()).ok();
        let flags = TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL | TPM_RETURNCMD;
        let chosen = TrackPopupMenuEx(menu, flags.0, corners[0].x, corners[1].y, a.hwnd, Some(&params)).0;
        if let Some(hook) = hook {
            let _ = UnhookWindowsHookEx(hook);
        }
        let _ = DestroyMenu(menu);
        chosen
    };
    (chosen as u16, TRACKING.with(|t| t.borrow_mut().take()))
}

/// `WM_MENUSELECT`: keep track of where the selection is, for Left and Right.
pub fn selected(wp: WPARAM, lp: LPARAM) {
    let flags = (wp.0 >> 16) as u32 & 0xFFFF;
    if flags == 0xFFFF && lp.0 == 0 {
        return;
    }
    TRACKING.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.popup = Popup {
                in_submenu: lp.0 != t.top.0 as isize,
                on_submenu_item: flags & MF_POPUP.0 != 0,
            };
        }
    });
}

/// The message hook while a popup is open: Left and Right past its edges, and
/// the mouse over another menu button, move to that menu.
unsafe extern "system" fn filter(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == MSGF_MENU as i32 {
            let msg = &*(lp.0 as *const MSG);
            if let Some(to) = switch_to(msg) {
                TRACKING.with(|t| {
                    if let Some(t) = t.borrow_mut().as_mut() {
                        t.next = Some(to);
                    }
                });
                let _ = EndMenu();
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

fn switch_to(msg: &MSG) -> Option<(usize, bool)> {
    let (at, popup) = TRACKING.with(|t| t.borrow().as_ref().map(|t| (t.at, t.popup)))?;
    match msg.message {
        WM_KEYDOWN => {
            let key = key_of(msg.wParam.0 as u16);
            if key == Key::Esc && !popup.in_submenu {
                TRACKING.with(|t| {
                    if let Some(t) = t.borrow_mut().as_mut() {
                        t.escaped = true;
                    }
                });
            }
            in_popup(at, key, popup).map(|to| (to, true))
        }
        WM_MOUSEMOVE => {
            let a = app()?;
            let mut pt = msg.pt;
            unsafe {
                let _ = ScreenToClient(a.hwnd, &mut pt);
            }
            match a.ui.try_borrow().ok()?.layout.hit_test(pt.x, pt.y) {
                Hit::Menu(to) if to != at => Some((to, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The popup for `items`, or `None` if Windows refused one.
fn build(items: &[Item]) -> Option<HMENU> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            let added = match item {
                Item::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                Item::Command { label, id, enabled, checked } => {
                    let label = wide(label);
                    let mut info = MENUITEMINFOW {
                        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_ID | MIIM_STRING | MIIM_STATE | MIIM_FTYPE,
                        fType: if *checked { MFT_RADIOCHECK } else { MFT_STRING },
                        fState: state_flags(*enabled, *checked),
                        wID: *id as u32,
                        dwTypeData: PWSTR(label.as_ptr() as *mut u16),
                        ..Default::default()
                    };
                    info.cch = label.len() as u32 - 1;
                    InsertMenuItemW(menu, u32::MAX, true, &info)
                }
                Item::Sub { label, enabled, items } => {
                    let Some(sub) = build(items) else { continue };
                    let label = wide(label);
                    let info = MENUITEMINFOW {
                        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_SUBMENU | MIIM_STRING | MIIM_STATE,
                        fState: state_flags(*enabled, false),
                        hSubMenu: sub,
                        dwTypeData: PWSTR(label.as_ptr() as *mut u16),
                        cch: label.len() as u32 - 1,
                        ..Default::default()
                    };
                    InsertMenuItemW(menu, u32::MAX, true, &info)
                }
            };
            if added.is_err() {
                let _ = DestroyMenu(menu);
                return None;
            }
        }
        Some(menu)
    }
}

fn state_flags(enabled: bool, checked: bool) -> MENU_ITEM_STATE {
    let mut state = if enabled { MFS_ENABLED } else { MFS_DISABLED };
    if checked {
        state |= MFS_CHECKED;
    }
    state
}

/// What the popups depend on, read now.
fn state_of(a: &App) -> State {
    let tab = {
        let tabs = a.tabs.borrow();
        tabs.active().and_then(|id| tabs.get(id)).map(|t| (t.encoding(), t.eol(), t.read_only(), t.is_untitled()))
    };
    let (encoding, eol, read_only, untitled) = tab.unwrap_or((NEW_FILE.0, NEW_FILE.1, false, true));
    let ask = |msg| editor_call(a, msg) != 0;
    State {
        encoding,
        eol,
        read_only,
        untitled,
        can_undo: ask(sci::SCI_CANUNDO),
        can_redo: ask(sci::SCI_CANREDO),
        selection: !ask(sci::SCI_GETSELECTIONEMPTY),
        can_paste: ask(sci::SCI_CANPASTE),
    }
}

fn editor_call(a: &App, msg: u32) -> isize {
    unsafe { SendMessageW(a.editor.hwnd(), msg, Some(WPARAM(0)), Some(LPARAM(0))).0 }
}

/// A menu command (or its shortcut) as `WM_COMMAND`. Returns false for an id
/// that is not a menu's.
pub fn run(a: &App, id: u16) -> bool {
    let active = a.tabs.borrow().active();
    let on_active = |f: &dyn Fn(&mut Tabs, TabId) -> Vec<Action>| {
        if let Some(tab) = active {
            command(a, |t| f(t, tab));
        }
    };
    match id {
        cmd::OPEN => files::open(a),
        cmd::SAVE => on_active(&|t, id| t.save(id)),
        cmd::SAVE_AS => on_active(&|t, id| t.save_as(id)),
        cmd::EXIT => unsafe {
            let _ = PostMessageW(Some(a.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        cmd::UNDO => drop(editor_call(a, sci::SCI_UNDO)),
        cmd::REDO => drop(editor_call(a, sci::SCI_REDO)),
        cmd::CUT => drop(editor_call(a, sci::SCI_CUT)),
        cmd::COPY => drop(editor_call(a, sci::SCI_COPY)),
        cmd::PASTE => drop(editor_call(a, sci::SCI_PASTE)),
        cmd::SELECT_ALL => drop(editor_call(a, sci::SCI_SELECTALL)),
        _ if (cmd::ENCODING..cmd::ENCODING + Encoding::MENU.len() as u16).contains(&id) => {
            let encoding = Encoding::MENU[(id - cmd::ENCODING) as usize];
            on_active(&|t, tab| t.set_encoding(tab, encoding));
        }
        cmd::ENCODING_KEEP => {}
        cmd::REOPEN => files::reopen(a),
        _ if (cmd::EOL..cmd::EOL + Eol::MENU.len() as u16).contains(&id) => {
            let eol = Eol::MENU[(id - cmd::EOL) as usize];
            on_active(&|t, tab| t.set_eol(tab, eol));
        }
        // 12 and 13 wire these; their items are greyed until then.
        cmd::FIND | cmd::REPLACE | cmd::GO_TO_LINE => {}
        cmd::RENDERED_VIEW | cmd::WORD_WRAP | cmd::ZOOM_IN | cmd::ZOOM_OUT | cmd::ZOOM_RESET => {}
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::cmd;

    fn state() -> State {
        State {
            encoding: Encoding::Utf8,
            eol: Eol::Lf,
            read_only: false,
            untitled: false,
            can_undo: true,
            can_redo: true,
            selection: true,
            can_paste: true,
        }
    }

    /// Labels without access-key ampersands, separators as "-".
    fn labels(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                Item::Separator => "-".to_string(),
                Item::Command { label, .. } | Item::Sub { label, .. } => label.replace('&', ""),
            })
            .collect()
    }

    fn sub(items: &[Item], name: &str) -> (bool, Vec<Item>) {
        items
            .iter()
            .find_map(|i| match i {
                Item::Sub { label, enabled, items } if label.replace('&', "") == name => Some((*enabled, items.clone())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {name} submenu"))
    }

    fn command(items: &[Item], id: u16) -> &Item {
        items.iter().find(|i| matches!(i, Item::Command { id: c, .. } if *c == id)).expect("no such command")
    }

    fn enabled(items: &[Item], id: u16) -> bool {
        matches!(command(items, id), Item::Command { enabled: true, .. })
    }

    fn checked(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Command { label, checked: true, .. } => Some(label.replace('&', "")),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_file_menu_reads_as_the_spec_lists_it() {
        assert_eq!(
            labels(&items(FILE, &state())),
            [
                "New tab\tCtrl+T",
                "Open…\tCtrl+O",
                "Recent files",
                "-",
                "Save\tCtrl+S",
                "Save as…\tCtrl+Shift+S",
                "-",
                "Encoding",
                "Line endings",
                "-",
                "Close tab\tCtrl+W",
                "Exit",
            ]
        );
    }

    #[test]
    fn recent_files_is_an_empty_disabled_submenu_for_now() {
        let (enabled, items) = sub(&items(FILE, &state()), "Recent files");
        assert!(!enabled);
        assert!(items.is_empty());
    }

    #[test]
    fn the_encoding_submenu_checks_the_tabs_encoding_and_ends_with_reopen() {
        let s = State { encoding: Encoding::Utf16Le, ..state() };
        let (enabled, items) = sub(&items(FILE, &s), "Encoding");
        assert!(enabled);
        assert_eq!(
            labels(&items),
            ["UTF-8", "UTF-8 with BOM", "UTF-16 LE", "UTF-16 BE", "Windows-1252 (ANSI)", "-", "Reopen with encoding…"]
        );
        assert_eq!(checked(&items), ["UTF-16 LE"]);
        assert!(enabled_all(&items));
    }

    fn enabled_all(items: &[Item]) -> bool {
        items.iter().all(|i| !matches!(i, Item::Command { enabled: false, .. }))
    }

    #[test]
    fn an_encoding_outside_the_menu_shows_checked_at_the_top() {
        let s = State { encoding: Encoding::Other(encoding_rs::SHIFT_JIS), ..state() };
        let (_, items) = sub(&items(FILE, &s), "Encoding");
        assert_eq!(labels(&items)[..2], ["Shift_JIS".to_string(), "-".to_string()]);
        assert_eq!(checked(&items), ["Shift_JIS"]);
    }

    #[test]
    fn a_read_only_tab_can_only_reopen_with_another_encoding() {
        let s = State { read_only: true, ..state() };
        let file = items(FILE, &s);
        let (_, encodings) = sub(&file, "Encoding");
        for (i, _) in Encoding::MENU.iter().enumerate() {
            assert!(!enabled(&encodings, cmd::ENCODING + i as u16));
        }
        assert!(enabled(&encodings, cmd::REOPEN));
        let (_, eols) = sub(&file, "Line endings");
        assert!(!enabled_all(&eols));
    }

    #[test]
    fn an_untitled_tab_has_no_file_to_reopen() {
        let s = State { untitled: true, ..state() };
        let (_, items) = sub(&items(FILE, &s), "Encoding");
        assert!(!enabled(&items, cmd::REOPEN));
    }

    #[test]
    fn the_line_endings_submenu_checks_the_tabs_line_ending() {
        let s = State { eol: Eol::Crlf, ..state() };
        let (_, items) = sub(&items(FILE, &s), "Line endings");
        assert_eq!(labels(&items), ["LF", "CRLF", "CR"]);
        assert_eq!(checked(&items), ["CRLF"]);
    }

    #[test]
    fn the_edit_menu_has_the_clipboard_and_undo_commands() {
        let edit = items(EDIT, &state());
        assert_eq!(
            labels(&edit)[..7],
            [
                "Undo\tCtrl+Z",
                "Redo\tCtrl+Y",
                "-",
                "Cut\tCtrl+X",
                "Copy\tCtrl+C",
                "Paste\tCtrl+V",
                "Select all\tCtrl+A",
            ]
        );
        for id in [cmd::UNDO, cmd::REDO, cmd::CUT, cmd::COPY, cmd::PASTE, cmd::SELECT_ALL] {
            assert!(enabled(&edit, id));
        }
    }

    #[test]
    fn edit_commands_follow_the_editor() {
        let s = State { can_undo: false, can_redo: false, selection: false, can_paste: false, ..state() };
        let edit = items(EDIT, &s);
        for id in [cmd::UNDO, cmd::REDO, cmd::CUT, cmd::COPY, cmd::PASTE] {
            assert!(!enabled(&edit, id), "{id}");
        }
        assert!(enabled(&edit, cmd::SELECT_ALL));
        // Read-only text can be copied, not cut or pasted into.
        let edit = items(EDIT, &State { read_only: true, ..state() });
        assert!(enabled(&edit, cmd::COPY));
        assert!(!enabled(&edit, cmd::CUT));
        assert!(!enabled(&edit, cmd::PASTE));
    }

    #[test]
    fn find_and_the_view_menu_wait_for_their_tickets_greyed() {
        let edit = items(EDIT, &state());
        assert_eq!(labels(&edit)[7..], ["-", "Find…\tCtrl+F", "Replace…\tCtrl+H", "Go to line…\tCtrl+G"]);
        for id in [cmd::FIND, cmd::REPLACE, cmd::GO_TO_LINE] {
            assert!(!enabled(&edit, id));
        }
        let view = items(VIEW, &state());
        assert_eq!(
            labels(&view),
            [
                "Rendered view\tCtrl+E",
                "Word wrap\tAlt+Z",
                "-",
                "Zoom in\tCtrl++",
                "Zoom out\tCtrl+\u{2212}",
                "Reset zoom\tCtrl+0"
            ]
        );
        assert!(!enabled_all(&view));
    }

    /// Every command id, submenus included.
    fn ids(items: &[Item], out: &mut Vec<u16>) {
        for i in items {
            match i {
                Item::Command { id, .. } => out.push(*id),
                Item::Sub { items, .. } => ids(items, out),
                Item::Separator => {}
            }
        }
    }

    #[test]
    fn command_ids_are_distinct_and_clear_of_the_tab_shortcuts() {
        let mut all = Vec::new();
        let s = State { encoding: Encoding::Other(encoding_rs::SHIFT_JIS), ..state() };
        for menu in MENUS {
            ids(&items(menu, &s), &mut all);
        }
        for tab in [cmd::NEW_TAB, cmd::CLOSE_TAB] {
            all.retain(|id| *id != tab);
        }
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "{all:?}");
        assert!(!all.contains(&cmd::NEXT_TAB) && !all.contains(&cmd::PREV_TAB));
    }

    fn access_keys(items: &[Item]) -> Vec<char> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Command { label, .. } | Item::Sub { label, .. } => {
                    let at = label.find('&')?;
                    label[at + 1..].chars().next().map(|c| c.to_ascii_lowercase())
                }
                Item::Separator => None,
            })
            .collect()
    }

    #[test]
    fn every_item_has_an_access_key_unique_in_its_popup() {
        let s = state();
        let file = items(FILE, &s);
        let mut popups = vec![file.clone(), items(EDIT, &s), items(VIEW, &s)];
        popups.push(sub(&file, "Encoding").1);
        popups.push(sub(&file, "Line endings").1);
        for popup in popups {
            let keys = access_keys(&popup);
            let named = popup.iter().filter(|i| !matches!(i, Item::Separator)).count();
            assert_eq!(keys.len(), named, "{:?}", labels(&popup));
            let mut unique = keys.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), keys.len(), "{:?}", labels(&popup));
        }
    }

    // --- The menu bar from the keyboard ------------------------------------

    #[test]
    fn alt_f_e_and_v_name_the_three_menus() {
        assert_eq!(access_key('f'), Some(0));
        assert_eq!(access_key('E'), Some(1));
        assert_eq!(access_key('v'), Some(2));
        assert_eq!(access_key('x'), None);
        assert_eq!(access_key(' '), None);
    }

    #[test]
    fn arrows_move_across_the_bar_and_wrap() {
        assert_eq!(on_bar(0, Key::Right), Bar::Focus(1));
        assert_eq!(on_bar(2, Key::Right), Bar::Focus(0));
        assert_eq!(on_bar(0, Key::Left), Bar::Focus(2));
        assert_eq!(on_bar(1, Key::Left), Bar::Focus(0));
    }

    #[test]
    fn enter_and_up_or_down_open_the_highlighted_menu() {
        for key in [Key::Enter, Key::Down, Key::Up] {
            assert_eq!(on_bar(1, key), Bar::Open(1));
        }
    }

    #[test]
    fn a_menus_letter_opens_it() {
        assert_eq!(on_bar(0, Key::Letter('v')), Bar::Open(2));
        assert_eq!(on_bar(2, Key::Letter('F')), Bar::Open(0));
    }

    #[test]
    fn esc_alt_and_f10_leave_the_bar() {
        for key in [Key::Esc, Key::Alt, Key::F10] {
            assert_eq!(on_bar(1, key), Bar::Leave);
        }
    }

    #[test]
    fn other_keys_keep_the_bar_highlighted() {
        assert_eq!(on_bar(1, Key::Letter('q')), Bar::Focus(1));
        assert_eq!(on_bar(1, Key::Other), Bar::Focus(1));
    }

    #[test]
    fn in_an_open_menu_left_and_right_move_to_the_next_menu() {
        let top = Popup { in_submenu: false, on_submenu_item: false };
        assert_eq!(in_popup(0, Key::Right, top), Some(1));
        assert_eq!(in_popup(0, Key::Left, top), Some(2));
        assert_eq!(in_popup(1, Key::Down, top), None);
    }

    #[test]
    fn right_on_a_submenu_item_opens_the_submenu_instead() {
        let on_sub = Popup { in_submenu: false, on_submenu_item: true };
        assert_eq!(in_popup(0, Key::Right, on_sub), None);
        assert_eq!(in_popup(0, Key::Left, on_sub), Some(2));
    }

    #[test]
    fn left_in_a_submenu_closes_it_and_right_moves_on() {
        let inside = Popup { in_submenu: true, on_submenu_item: false };
        assert_eq!(in_popup(0, Key::Left, inside), None);
        assert_eq!(in_popup(0, Key::Right, inside), Some(1));
    }
}
