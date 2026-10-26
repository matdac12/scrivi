//! Every WM_APP + n message scrivi posts, in one place so numbers never collide.

use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// The Window's class. A later launch finds the resident by it.
pub const CLASS: PCWSTR = w!("Scrivi.Window");
/// `MAKEINTRESOURCE(1)`: the app icon in `assets/scrivi.rc`.
pub const ICON: PCWSTR = PCWSTR(std::ptr::without_provenance(1));
/// Held by the resident for its whole life.
pub const MUTEX: PCWSTR = w!("Local\\Scrivi.Resident");

/// `COPYDATASTRUCT::dwData` of a launch request ("SCR1"). Anything else is ignored.
pub const COPYDATA_REQUEST: usize = 0x5343_5231;

// Internal resident messages: every WM_APP + n goes here.

/// Posted from `WM_DPICHANGED`: Scintilla has seen the new DPI by the time it
/// arrives, so the editor can re-measure its pixel sizes.
pub const WM_EDITOR_DPI: u32 = WM_APP + 1;
/// Posted from `WM_COPYDATA` so the launch gets its answer at once; `LPARAM`
/// is a `Box<launch::Request>` from `Box::into_raw`.
pub const WM_HANDOFF: u32 = WM_APP + 2;

#[cfg(test)]
mod tests {
    #[test]
    fn message_numbers_are_distinct() {
        let all = [super::WM_EDITOR_DPI, super::WM_HANDOFF];
        for (i, a) in all.iter().enumerate() {
            assert!(all[i + 1..].iter().all(|b| a != b), "{a:#x} used twice");
        }
    }
}
