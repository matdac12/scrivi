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

use super::findview::Zoom;
use super::{app, cmd, command, files, find, findview, invalidate, recent, rehover, wide, App};
use crate::chrome::findbar::Mode as BarMode;
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
    /// A Markdown file or Untitled tab: Ctrl+E applies (13).
    pub markdown: bool,
    /// The Rendered view shows: there is no text to search or jump in (12).
    pub rendered: bool,
    /// View › Word wrap: on, and whether the Tab allows it (a Large file doesn't).
    pub wrap: bool,
    pub wrap_allowed: bool,
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

/// The Source view shows.
fn source(s: &State) -> bool {
    !s.rendered
}

fn writable(s: &State) -> bool {
    !s.read_only
}

pub const FILE: &[Entry] = &[
    Entry::Command { label: "&New tab", keys: "Ctrl+T", id: cmd::NEW_TAB, enabled: always },
    Entry::Command { label: "&Open\u{2026}", keys: "Ctrl+O", id: cmd::OPEN, enabled: always },
    Entry::Sub { label: "Recent &files", enabled: |_| recent::any(), items: |_| recent::menu_items() },
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
    Entry::Command { label: "&Find\u{2026}", keys: "Ctrl+F", id: cmd::FIND, enabled: source },
    Entry::Command { label: "R&eplace\u{2026}", keys: "Ctrl+H", id: cmd::REPLACE, enabled: source },
    Entry::Command { label: "&Go to line\u{2026}", keys: "Ctrl+G", id: cmd::GO_TO_LINE, enabled: source },
];

pub const VIEW: &[Entry] = &[
    Entry::Command { label: "&Rendered view", keys: "Ctrl+E", id: cmd::RENDERED_VIEW, enabled: |s| s.markdown },
    Entry::Command { label: "&Word wrap", keys: "Alt+Z", id: cmd::WORD_WRAP, enabled: |s| s.wrap_allowed },
    Entry::Separator,
    Entry::Command { label: "Zoom &in", keys: "Ctrl++", id: cmd::ZOOM_IN, enabled: always },
    Entry::Command { label: "Zoom &out", keys: "Ctrl+\u{2212}", id: cmd::ZOOM_OUT, enabled: always },
    Entry::Command { label: "Reset &zoom", keys: "Ctrl+0", id: cmd::ZOOM_RESET, enabled: always },
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
                // Word wrap is the one check mark; the others are radio choices.
                checked: *id == cmd::WORD_WRAP && state.wrap && state.wrap_allowed,
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
        let label = s.encoding.name().into();
        items.push(Item::Command { label, id: cmd::ENCODING_KEEP, enabled: true, checked: true });
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
    /// Where the mouse last was: only a move away from it switches menus,
    /// not the move Windows reports when a popup opens under a resting mouse.
    mouse: POINT,
    /// Opened from the keyboard: select the first item once the menu loop
    /// runs. Posting the key earlier would reach the editor if the popup
    /// never opened.
    select_first: bool,
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
        let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0);
        SystemParametersInfoW(SPI_GETKEYBOARDCUES, 0, Some(&mut on as *mut BOOL as *mut _), flags)
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
        // Alt+F, E or V. Scintilla never hands WM_SYSCHAR to DefWindowProc,
        // so no SC_KEYMENU would follow.
        WM_SYSCHAR if mode() == Mode::Idle => match char::from_u32(vk as u32).and_then(access_key) {
            Some(at) => {
                open(a, at, true);
                true
            }
            None => false,
        },
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
        // The mouse may have moved off the button while the popup held it.
        rehover(a);
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
            mouse: cursor(),
            select_first: keyboard,
        })
    });
    let chosen = unsafe {
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

/// `WM_ENTERMENULOOP`: the popup is open. Opened from the keyboard, a native
/// menu bar selects the first item, so send the key now and not before.
pub fn loop_entered(a: &App) {
    let first = TRACKING.with(|t| t.borrow_mut().as_mut().is_some_and(|t| std::mem::take(&mut t.select_first)));
    if first {
        unsafe {
            let _ = PostMessageW(Some(a.hwnd), WM_KEYDOWN, WPARAM(VK_DOWN.0 as usize), LPARAM(0));
        }
    }
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
            // Releasing Alt after Alt+F would end the menu loop; a native
            // menu bar's popup stays open.
            if matches!(msg.message, WM_KEYUP | WM_SYSKEYUP) && VIRTUAL_KEY(msg.wParam.0 as u16) == VK_MENU {
                return LRESULT(1);
            }
            if let Some(end) = ends_popup(msg) {
                TRACKING.with(|t| {
                    if let Some(t) = t.borrow_mut().as_mut() {
                        t.next = end;
                    }
                });
                let _ = EndMenu();
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

/// A message that ends the open popup: `Some(Some(menu))` moves to that menu
/// (`true`: by keyboard), `Some(None)` just closes it.
fn ends_popup(msg: &MSG) -> Option<Option<(usize, bool)>> {
    let (at, popup) = TRACKING.with(|t| t.borrow().as_ref().map(|t| (t.at, t.popup)))?;
    match msg.message {
        WM_KEYDOWN => {
            let key = key_of(msg.wParam.0 as u16);
            // Esc on a hovered "Encoding ›" closes only its submenu.
            if key == Key::Esc && !popup.in_submenu && !popup.on_submenu_item {
                TRACKING.with(|t| {
                    if let Some(t) = t.borrow_mut().as_mut() {
                        t.escaped = true;
                    }
                });
            }
            in_popup(at, key, popup).map(|to| Some((to, true)))
        }
        WM_MOUSEMOVE => {
            let moved = TRACKING.with(|t| {
                t.borrow_mut().as_mut().is_some_and(|t| std::mem::replace(&mut t.mouse, msg.pt) != msg.pt)
            });
            match button_at(msg.pt) {
                Some(to) if moved && to != at => Some(Some((to, false))),
                _ => None,
            }
        }
        // A click on the open menu's own button closes it, and goes no further.
        WM_LBUTTONDOWN if button_at(msg.pt) == Some(at) => Some(None),
        _ => None,
    }
}

/// The menu button under the screen point `pt`, if any.
fn button_at(mut pt: POINT) -> Option<usize> {
    let a = app()?;
    unsafe {
        let _ = ScreenToClient(a.hwnd, &mut pt);
    }
    match a.ui.try_borrow().ok()?.layout.hit_test(pt.x, pt.y) {
        Hit::Menu(i) => Some(i),
        _ => None,
    }
}

fn cursor() -> POINT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt
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
                        fType: if *checked && *id != cmd::WORD_WRAP { MFT_RADIOCHECK } else { MFT_STRING },
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
                    let added = InsertMenuItemW(menu, u32::MAX, true, &info);
                    if added.is_err() {
                        let _ = DestroyMenu(sub);
                    }
                    added
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
        tabs.active().and_then(|id| tabs.get(id)).map(|t| {
            // The Rendered view is read-only too (13).
            let rendered = t.view() == crate::tabs::View::Rendered;
            let read_only = t.read_only() || rendered;
            (t.encoding(), t.eol(), read_only, t.is_untitled(), t.kind() == crate::tabs::Kind::Markdown, rendered)
        })
    };
    let wrap_allowed = findview::wrap_allowed(a);
    let (encoding, eol, read_only, untitled, markdown, rendered) =
        tab.unwrap_or((NEW_FILE.0, NEW_FILE.1, false, true, true, false));
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
        markdown,
        rendered,
        wrap: findview::wrap_wanted(),
        wrap_allowed,
    }
}

fn editor_call(a: &App, msg: u32) -> isize {
    files::send(&a.editor, msg)
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
        cmd::UNDO | cmd::REDO | cmd::CUT | cmd::COPY | cmd::PASTE | cmd::SELECT_ALL => {
            let msg = match id {
                cmd::UNDO => sci::SCI_UNDO,
                cmd::REDO => sci::SCI_REDO,
                cmd::CUT => sci::SCI_CUT,
                cmd::COPY => sci::SCI_COPY,
                cmd::PASTE => sci::SCI_PASTE,
                _ => sci::SCI_SELECTALL,
            };
            editor_call(a, msg);
        }
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
        // The accelerators reach here even when the menu item is greyed.
        cmd::FIND | cmd::REPLACE | cmd::GO_TO_LINE if state_of(a).rendered => {}
        cmd::FIND => find::open(a, BarMode::Find),
        cmd::REPLACE => find::open(a, BarMode::Replace),
        cmd::GO_TO_LINE => find::open(a, BarMode::Goto),
        cmd::WORD_WRAP => findview::toggle_wrap(a),
        cmd::ZOOM_IN => findview::zoom(a, Zoom::In),
        cmd::ZOOM_OUT => findview::zoom(a, Zoom::Out),
        cmd::ZOOM_RESET => findview::zoom(a, Zoom::Reset),
        // 13's RENDERED_VIEW is routed by `app` before the menus.
        _ if recent::is_command(id) => recent::open(a, id),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests;
