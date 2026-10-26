//! The Notice bar of the active Tab (SCRIVI-18): which one shows, and what a
//! click on it asks. `tabs` owns the Notice; only the mouse over it is kept
//! here, since the band is drawn from a [`Spec`] rebuilt for every paint.

use std::cell::Cell;

use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};

use super::{command, invalidate, App};
use crate::chrome::notice::{self, Hit, Mouse, Parts, Spec};
use crate::tabs::Tabs;

thread_local! {
    static MOUSE: Cell<Mouse> = const { Cell::new(Mouse { over: None, down: None }) };
}

/// The Notice the active Tab shows, if any, with the mouse over it.
pub fn spec(tabs: &Tabs) -> Option<Spec> {
    let tab = tabs.active().and_then(|id| tabs.get(id))?;
    let mut spec = notice::for_tab(tab.notice()?, &tab.name(), tab.file_size());
    spec.mouse = MOUSE.get();
    Some(spec)
}

/// The active Tab's Notice and where its pieces sit, if one shows.
fn parts(a: &App) -> Option<(Spec, Parts)> {
    let spec = spec(&a.tabs.borrow())?;
    let band = a.ui.borrow().layout.notice;
    let parts = a.painter.borrow().notice_parts(band, &spec);
    Some((spec, parts))
}

fn set(a: &App, mouse: Mouse) {
    if MOUSE.replace(mouse) != mouse {
        let band = a.ui.borrow().layout.notice;
        invalidate(a, Some(band));
    }
}

/// What the Notice has at client point `(x, y)`.
fn over(a: &App, x: i32, y: i32) -> Option<Hit> {
    parts(a).and_then(|(_, parts)| parts.hit(x, y))
}

/// The mouse moved, anywhere in the Window.
pub fn moved(a: &App, x: i32, y: i32) {
    set(a, Mouse { over: over(a, x, y), ..MOUSE.get() });
}

/// The mouse left the Window.
pub fn left(a: &App) {
    set(a, Mouse { over: None, ..MOUSE.get() });
}

/// The mouse went down at `(x, y)` in the Notice band: a button or the ×
/// shows pressed, and holds the mouse until it comes up.
pub fn press(a: &App, x: i32, y: i32) {
    let Some(hit) = over(a, x, y) else { return };
    set(a, Mouse { over: Some(hit), down: Some(hit) });
    unsafe {
        SetCapture(a.hwnd);
    }
}

/// The press ended without a click (the capture was lost).
pub fn cancel(a: &App) {
    set(a, Mouse { down: None, ..MOUSE.get() });
}

/// The mouse came up at `(x, y)`. Ask the Tab for the button's action only if
/// that is the button it went down on.
pub fn release(a: &App, x: i32, y: i32) {
    let down = MOUSE.get().down;
    if down.is_none() {
        return;
    }
    let at = over(a, x, y);
    set(a, Mouse { over: at, down: None });
    unsafe {
        if GetCapture() == a.hwnd {
            // Sends WM_CAPTURECHANGED: nothing may be borrowed here.
            let _ = ReleaseCapture();
        }
    }
    let Some(hit) = Mouse::released_on(down, at) else { return };
    let Some((spec, _)) = parts(a) else { return };
    let Some(id) = a.tabs.borrow().active() else { return };
    if let Some(action) = spec.action(hit) {
        command(a, |t| t.notice_action(id, action));
    }
}
