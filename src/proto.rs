//! Every WM_APP + n message scrivi posts, in one place so numbers never collide.

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// The Window's class. A later launch finds the resident by it.
pub fn class() -> PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    instanced(&NAME, "Scrivi.Window")
}

/// Held by the resident for its whole life.
pub fn mutex() -> PCWSTR {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    instanced(&NAME, "Local\\Scrivi.Resident")
}

/// `MAKEINTRESOURCE(1)`: the app icon in `assets/scrivi.rc`.
pub const ICON: PCWSTR = PCWSTR(std::ptr::without_provenance(1));

/// Debug builds only: `SCRIVI_TEST_INSTANCE` names a resident of its own, so
/// integration tests never meet another worktree's resident or a running
/// scrivi. Release builds ignore it.
fn test_instance() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("SCRIVI_TEST_INSTANCE").ok()
    } else {
        None
    }
}

fn instanced(cell: &'static OnceLock<Vec<u16>>, base: &str) -> PCWSTR {
    let name = cell.get_or_init(|| name(base, test_instance().as_deref()).encode_utf16().chain(Some(0)).collect());
    PCWSTR(name.as_ptr())
}

/// `base`, then `.instance`, with anything but ASCII letters, digits and `-`
/// in it made `_` (a mutex name holds one backslash, after `Local`).
fn name(base: &str, instance: Option<&str>) -> String {
    match instance.filter(|i| !i.is_empty()) {
        Some(i) => {
            let safe: String = i.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
            format!("{base}.{safe}")
        }
        None => base.to_string(),
    }
}

/// `COPYDATASTRUCT::dwData` of a launch request ("SCR1"). Anything else is ignored.
pub const COPYDATA_REQUEST: usize = 0x5343_5231;

/// Timer ids on the Window. Flash is 1; Recheck timers count up from 0x100.
pub const TIMER_RESTYLE: usize = 2;
/// Quiet time after an edit before markdown styling is redone.
pub const RESTYLE_MS: u32 = 150;
/// Spell check runs after this much quiet, after an edit or a scroll.
pub const TIMER_SPELL: usize = 3;
pub const SPELL_MS: u32 = 200;
/// Prefs are written after the Window has been still for a moment (SCRIVI-23).
pub const TIMER_PREFS: usize = 0x20;
/// The find bar's matches are found again once typing in the document pauses.
pub const TIMER_FIND: usize = 0x30;
pub const FIND_MS: u32 = 150;

// Internal resident messages: every WM_APP + n goes here.

/// Posted from `WM_DPICHANGED`: Scintilla has seen the new DPI by the time it
/// arrives, so the editor can re-measure its pixel sizes.
pub const WM_EDITOR_DPI: u32 = WM_APP + 1;
/// Posted from `WM_COPYDATA` so the launch gets its answer at once; `LPARAM`
/// carries nothing: the request waits in a queue on the UI thread.
pub const WM_HANDOFF: u32 = WM_APP + 2;

// 13, the Rendered view, posts from WebView2's handlers: numbers from 0x130.

/// The Rendered view's page has loaded: `rendered::webview::reveal` shows it.
pub const WM_RENDERED_SHOWN: u32 = WM_APP + 0x130;
/// A link clicked in the Rendered view; carries nothing: the URI waits in
/// `rendered::webview`'s queue, checked again by `rendered::opens_outside`.
pub const WM_RENDERED_LINK: u32 = WM_APP + 0x131;
/// WebView2 couldn't start or show the page; `WPARAM` is
/// `rendered::webview::FAILED_TO_START` or `FAILED_TOO_LARGE`.
pub const WM_RENDERED_FAILED: u32 = WM_APP + 0x132;

// 11, Disk changes, posts from the watcher's thread: numbers from 0x190.

/// Watched files may have changed: `app::disk` holds them in a queue (no
/// pointer travels). The UI thread stats each file again before acting.
pub const WM_DISK_EVENT: u32 = WM_APP + 0x190;

#[cfg(test)]
mod tests {
    use super::name;

    #[test]
    fn a_test_instance_suffixes_the_class_and_mutex_names() {
        assert_eq!(name("Scrivi.Window", None), "Scrivi.Window");
        assert_eq!(name("Scrivi.Window", Some("")), "Scrivi.Window");
        assert_eq!(name("Local\\Scrivi.Resident", Some("4242-save")), "Local\\Scrivi.Resident.4242-save");
        assert_eq!(name("Scrivi.Window", Some("a\\b c")), "Scrivi.Window.a_b_c");
    }

    #[test]
    fn message_numbers_are_distinct() {
        let all = [
            super::WM_EDITOR_DPI,
            super::WM_HANDOFF,
            super::WM_RENDERED_SHOWN,
            super::WM_RENDERED_LINK,
            super::WM_RENDERED_FAILED,
            super::WM_DISK_EVENT,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(all[i + 1..].iter().all(|b| a != b), "{a:#x} used twice");
        }
    }
}
