//! Disk changes (SCRIVI-19): the watcher's events arriving at the Window.
//! The debouncer's thread queues each one and posts a bare [`WM_DISK_EVENT`]
//! (no pointer in `LPARAM`); here, on the UI thread, each file is stat-ed
//! again and `tabs` decides what it means.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{command, stat_of, App};
use crate::proto::WM_DISK_EVENT;
use crate::watch::{Change, Watcher};

/// Events the watcher found, waiting for the UI thread. A message is posted
/// only when this was empty, so one is always on its way while it is not.
static EVENTS: Mutex<Vec<(PathBuf, Change)>> = Mutex::new(Vec::new());

/// A watcher whose events reach `hwnd`.
pub fn watcher(hwnd: HWND) -> Watcher {
    let hwnd = hwnd.0 as isize;
    Watcher::new(move |path, change| {
        let first = {
            let Ok(mut events) = EVENTS.lock() else {
                return;
            };
            events.push((path, change));
            events.len() == 1
        };
        if first {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_DISK_EVENT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    })
}

pub fn watch(a: &App, path: &Path) {
    a.watcher.borrow_mut().watch(path);
}

pub fn unwatch(a: &App, path: &Path) {
    a.watcher.borrow_mut().unwatch(path);
}

/// `WM_DISK_EVENT`: files may have changed. What an event said is not
/// trusted (a rename pair or a replace burst says little): the file as it is
/// now decides.
pub fn events(a: &App) {
    let events = EVENTS
        .lock()
        .map(|mut e| std::mem::take(&mut *e))
        .unwrap_or_default();
    for (path, _) in events {
        let on_disk = stat_of(&path);
        command(a, |t| t.disk_event(&path, on_disk, Instant::now()));
    }
    #[cfg(debug_assertions)]
    super::log_editor(a);
}
