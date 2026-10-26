//! View › Word wrap and Zoom (spec §11). `ViewOptions` is the one place the
//! remembered view settings live: `prefs` (15) reads [`options`] when it saves
//! and calls [`apply`] at start-up.
//!
//! Zoom is Scintilla's own (`SCI_SETZOOM`, in points on top of the font size)
//! and follows the control across Tabs; Ctrl+mouse wheel is Scintilla's too and
//! arrives as `SCN_ZOOM`, like the menu's steps. The status bar shows it when
//! it isn't 100 %.

use std::cell::Cell;

use super::{apply_settings, App};

/// What of the view is remembered across runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewOptions {
    /// Word wrap, where a Tab allows it (not for Large files).
    pub wrap: bool,
    /// Zoom in points added to the font size; 0 is 100 %.
    pub zoom_points: i32,
}

impl Default for ViewOptions {
    /// Wrap on, no zoom (spec §5).
    fn default() -> Self {
        ViewOptions { wrap: true, zoom_points: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

thread_local! {
    static WRAP: Cell<bool> = const { Cell::new(true) };
}

/// The wrap the user asked for; a Tab that doesn't allow it stays unwrapped.
pub fn wrap_wanted() -> bool {
    WRAP.with(Cell::get)
}

/// The view settings as they are now.
#[allow(dead_code)] // 15 (prefs) saves them with this.
pub fn options(a: &App) -> ViewOptions {
    ViewOptions { wrap: wrap_wanted(), zoom_points: a.editor.zoom_points() }
}

/// Take `o` as the view settings.
#[allow(dead_code)] // 15 (prefs) restores them with this.
pub fn apply(a: &App, o: ViewOptions) {
    WRAP.with(|w| w.set(o.wrap));
    a.editor.set_zoom_points(o.zoom_points);
    reapply_wrap(a);
}

/// Alt+Z, View › Word wrap. Does nothing on a Tab that doesn't allow wrap.
pub fn toggle_wrap(a: &App) {
    if !wrap_allowed(a) {
        return;
    }
    WRAP.with(|w| w.set(!w.get()));
    reapply_wrap(a);
}

/// The shown Tab allows word wrap.
pub fn wrap_allowed(a: &App) -> bool {
    let tabs = a.tabs.borrow();
    tabs.active().and_then(|id| tabs.get(id)).is_none_or(|t| t.wrap_allowed())
}

fn reapply_wrap(a: &App) {
    if let Some(id) = a.shown.get() {
        // `apply_settings` sends only what changed.
        a.applied.set(None);
        apply_settings(a, id);
    }
}

/// Ctrl++, Ctrl+−, Ctrl+0 and View › Zoom.
pub fn zoom(a: &App, z: Zoom) {
    match z {
        Zoom::In => a.editor.zoom_step(false),
        Zoom::Out => a.editor.zoom_step(true),
        Zoom::Reset => a.editor.set_zoom_points(0),
    }
}
