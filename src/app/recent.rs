//! The Window's remembered state and recent files (SCRIVI-23, spec §10):
//! the adapter between the pure `prefs` and the Window, the File › Recent
//! files menu and the taskbar jump list.
//!
//! The prefs live in a thread-local here, so `app.rs` only calls the small
//! functions below. The jump list is the system's own Recent category:
//! `SHAddToRecentDocs` for a file whose handler is our registered ProgID
//! (`--register`); clicking an entry runs that ProgID's open command, which
//! reaches the running Window through the launch handoff. Test and debug runs
//! with their own prefs never call the shell, so they leave Mattia's real
//! Recent list and jump list alone.

use std::cell::RefCell;
use std::path::Path;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::UI::Shell::{SHAddToRecentDocs, SHARD_PATHW};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::menu::Item;
use crate::editor::sci;
use super::{cmd, command, wide, App};
use crate::prefs::{self, Placement, Prefs};
use crate::proto::TIMER_PREFS;
use crate::tabs::{Action, StatusNote, Tabs};

const SCI_SETZOOM: u32 = 2373;
/// Quiet time after the Window moves or resizes before prefs are written.
const SAVE_MS: u32 = 800;

struct State {
    prefs: Prefs,
    /// The Window's rectangle when it was neither maximised nor minimised.
    normal: Option<Placement>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = s.get_or_insert_with(|| {
            let prefs = prefs::load();
            State { normal: prefs.window, prefs }
        });
        f(state)
    })
}

// --- Startup and exit ----------------------------------------------------------

/// Put the Window where it was, clamped to a visible monitor. Returns whether
/// it was maximised, for the first `ShowWindow`.
pub fn restore_window(hwnd: HWND) -> bool {
    let (window, maximized) = with(|s| (s.prefs.window, s.prefs.maximized));
    if let Some(want) = window.and_then(|w| prefs::clamp_to_work_areas(w, &work_areas())) {
        unsafe {
            let _ = SetWindowPos(hwnd, None, want.x, want.y, want.w, want.h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }
    // The Window now sits where it sits (the default if the monitor is gone):
    // that is what a write before the first move must keep, not the stale rectangle.
    note_normal(hwnd);
    maximized
}

/// Apply the remembered view at startup: zoom now, wrap through [`wrap`].
pub fn apply_view(a: &App) {
    let zoom = with(|s| s.prefs.zoom);
    a.editor.call(SCI_SETZOOM, zoom as usize, 0);
}

/// The remembered word wrap (View › Word wrap, `findview`).
pub fn wrap() -> bool {
    with(|s| s.prefs.wrap)
}

/// Remember a change of word wrap.
pub fn set_wrap(a: &App, wrap: bool) {
    with(|s| s.prefs.wrap = wrap);
    arm(a.hwnd);
}

/// SCN_ZOOM: remember the new zoom.
pub fn zoom_changed(a: &App) {
    let points = (a.editor.call(sci::SCI_GETZOOM, 0, 0) as i32).clamp(prefs::ZOOM_MIN, prefs::ZOOM_MAX);
    if with(|s| std::mem::replace(&mut s.prefs.zoom, points)) != points {
        arm(a.hwnd);
    }
}

/// `WM_MOVE` or `WM_SIZE`: note the rectangle and write prefs once it settles.
pub fn window_changed(hwnd: HWND) {
    note_normal(hwnd);
    arm(hwnd);
}

/// Remember the Window's rectangle, unless it is maximised or minimised.
fn note_normal(hwnd: HWND) {
    unsafe {
        if !IsZoomed(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_ok() {
                with(|s| s.normal = Some(Placement { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }));
            }
        }
    }
}

fn arm(hwnd: HWND) {
    unsafe {
        SetTimer(Some(hwnd), TIMER_PREFS, SAVE_MS, None);
    }
}

/// `TIMER_PREFS` fired, or the Window is closing: write the prefs now.
pub fn flush(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_PREFS);
        let maximized = IsZoomed(hwnd).as_bool();
        let snapshot = with(|s| {
            s.prefs.maximized = maximized;
            s.prefs.window = s.normal;
            s.prefs.clone()
        });
        prefs::save(&snapshot);
    }
}

/// The work area of every monitor, in screen pixels.
fn work_areas() -> Vec<Placement> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(lp.0 as *mut Vec<Placement>);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(m, &mut mi).as_bool() {
                let r = mi.rcWork;
                list.push(Placement { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top });
            }
        }
        true.into()
    }
    let mut list: Vec<Placement> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

// --- Recent files --------------------------------------------------------------

/// `Action::AddRecent`: a file was opened or saved.
pub fn add(a: &App, path: &Path) {
    with(|s| prefs::push_recent(&mut s.prefs.recent, path));
    // The system's Recent category, which is the jump list's "Recent".
    if !prefs::sandboxed() {
        let wide = wide(&path.display().to_string());
        unsafe {
            SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(wide.as_ptr() as *const _));
        }
    }
    flush(a.hwnd);
}

pub fn any() -> bool {
    with(|s| !s.prefs.recent.is_empty())
}

/// File › Recent files, newest first.
pub fn menu_items() -> Vec<Item> {
    with(|s| {
        s.prefs
            .recent
            .iter()
            .enumerate()
            .map(|(i, p)| Item::Command {
                label: prefs::menu_label(i, p),
                id: cmd::RECENT + i as u16,
                enabled: true,
                checked: false,
            })
            .collect()
    })
}

pub fn is_command(id: u16) -> bool {
    (cmd::RECENT..cmd::RECENT + prefs::MAX_RECENT as u16).contains(&id)
}

/// A recent file was picked: open it (which moves it to the top), or drop it
/// from the list with a short note if it is gone.
pub fn open(a: &App, id: u16) {
    let Some(path) = with(|s| s.prefs.recent.get((id - cmd::RECENT) as usize).cloned()) else { return };
    if std::fs::metadata(&path).is_ok() {
        command(a, |t: &mut Tabs| t.open(std::slice::from_ref(&path)));
    } else {
        with(|s| prefs::remove_recent(&mut s.prefs.recent, &path));
        flush(a.hwnd);
        command(a, |_| vec![Action::Flash(StatusNote::RecentGone)]);
    }
}
