//! Every WM_APP + n message scrivi posts, in one place so numbers never collide.

use windows::core::{w, PCWSTR};

/// The Window's class. A later launch finds the resident by it.
pub const CLASS: PCWSTR = w!("Scrivi.Window");
/// Held by the resident for its whole life.
pub const MUTEX: PCWSTR = w!("Local\\Scrivi.Resident");

/// `COPYDATASTRUCT::dwData` of a launch request ("SCR1"). Anything else is ignored.
pub const COPYDATA_REQUEST: usize = 0x5343_5231;

// Internal resident messages: every WM_APP + n goes here. None yet; the next
// free number is WM_APP + 1.

#[cfg(test)]
mod tests {
    #[test]
    fn message_numbers_are_distinct() {
        let all: [u32; 0] = [];
        for (i, a) in all.iter().enumerate() {
            assert!(all[i + 1..].iter().all(|b| a != b), "{a:#x} used twice");
        }
    }
}
