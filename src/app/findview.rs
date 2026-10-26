//! View › Word wrap and Zoom (spec §11). Both are remembered by `prefs` (23,
//! through `recent`): wrap is read and written there, and zoom is saved on
//! `SCN_ZOOM` and restored at start-up.
//!
//! Zoom is Scintilla's own (`SCI_SETZOOM`, in points on top of the font size)
//! and follows the control across Tabs; Ctrl+mouse wheel is Scintilla's too and
//! arrives as `SCN_ZOOM`, like the menu's steps. The status bar shows it when
//! it isn't 100 %.

use super::{apply_settings, recent, App};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// The wrap the user asked for; a Tab that doesn't allow it stays unwrapped.
pub fn wrap_wanted() -> bool {
    recent::wrap()
}

/// Alt+Z, View › Word wrap. Does nothing on a Tab that doesn't allow wrap.
pub fn toggle_wrap(a: &App) {
    if !wrap_allowed(a) {
        return;
    }
    recent::set_wrap(a, !wrap_wanted());
    reapply_wrap(a);
}

/// The shown Tab allows word wrap.
pub fn wrap_allowed(a: &App) -> bool {
    let tabs = a.tabs.borrow();
    tabs.active()
        .and_then(|id| tabs.get(id))
        .is_none_or(|t| t.wrap_allowed())
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
