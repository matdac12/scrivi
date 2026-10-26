//! The Notice bar of the active Tab (SCRIVI-18): which one shows, and what a
//! click on it asks. `tabs` owns the Notice; nothing is kept here.

use super::{command, App};
use crate::chrome::notice::{self, Spec};
use crate::tabs::Tabs;

/// The Notice the active Tab shows, if any.
pub fn spec(tabs: &Tabs) -> Option<Spec> {
    let tab = tabs.active().and_then(|id| tabs.get(id))?;
    Some(notice::for_tab(tab.notice()?, &tab.name(), tab.file_size()))
}

/// A click at client point `(x, y)` in the Notice band.
pub fn click(a: &App, x: i32, y: i32) {
    let (id, spec) = {
        let tabs = a.tabs.borrow();
        let Some(id) = tabs.active() else { return };
        let Some(spec) = spec(&tabs) else { return };
        (id, spec)
    };
    let band = a.ui.borrow().layout.notice;
    let parts = a.painter.borrow().notice_parts(band, &spec);
    if let Some(action) = parts.hit(x, y).and_then(|hit| spec.action(hit)) {
        command(a, |t| t.notice_action(id, action));
    }
}
