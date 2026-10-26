//! Launch decisions: one resident, every later launch hands over (spec §3).
//! `decide` is pure; the adapter below it owns the mutex, `FindWindow`,
//! `WM_COPYDATA` and the relaunch through `explorer.exe` when elevated.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetShellWindow, GetWindowThreadProcessId,
    SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA,
};

use crate::proto::{CLASS, COPYDATA_REQUEST, MUTEX};

/// What a launch asks the Window to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Request {
    /// An Untitled tab.
    NewTab,
    /// Open each path in its own Tab, or switch to its Tab if already open.
    Open(Vec<PathBuf>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Decision {
    Register,
    Unregister,
    HandOff(Request),
    BecomeResident(Request),
}

pub fn decide(args: &[String], resident_running: bool) -> Decision {
    let mut paths = Vec::new();
    for a in args {
        match a.as_str() {
            "--register" => return Decision::Register,
            "--unregister" => return Decision::Unregister,
            s if s.starts_with("--") || s.is_empty() => {}
            s => paths.push(PathBuf::from(s)),
        }
    }
    let request = if paths.is_empty() { Request::NewTab } else { Request::Open(paths) };
    if resident_running {
        Decision::HandOff(request)
    } else {
        Decision::BecomeResident(request)
    }
}

/// The `WM_COPYDATA` payload: UTF-16 paths separated by NUL; empty means `NewTab`.
pub fn encode(request: &Request) -> Vec<u16> {
    match request {
        Request::NewTab => Vec::new(),
        Request::Open(paths) => {
            let mut out = Vec::new();
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    out.push(0);
                }
                out.extend(p.as_os_str().encode_wide());
            }
            out
        }
    }
}

pub fn decode(payload: &[u16]) -> Request {
    let paths: Vec<PathBuf> = payload
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(OsString::from_wide(s)))
        .collect();
    if paths.is_empty() {
        Request::NewTab
    } else {
        Request::Open(paths)
    }
}

/// Paths are relative to the launching process's directory, which the resident
/// does not share.
fn absolutize(request: Request) -> Request {
    match request {
        Request::NewTab => Request::NewTab,
        Request::Open(paths) => Request::Open(
            paths.into_iter().map(|p| std::path::absolute(&p).unwrap_or(p)).collect(),
        ),
    }
}

fn find_resident() -> Option<HWND> {
    unsafe { FindWindowW(CLASS, PCWSTR::null()) }.ok()
}

/// Elevation of this process. Debug builds only: `SCRIVI_TEST_ALLOW_ELEVATED=1`
/// makes an elevated process behave as unelevated, so tests run on CI runners
/// (which are elevated). Release builds never read it.
fn is_elevated() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var_os("SCRIVI_TEST_ALLOW_ELEVATED").is_some_and(|v| v == "1") {
        return false;
    }
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// Start `explorer.exe <this exe>`: Explorer launches it unelevated.
fn relaunch_through_shell() -> bool {
    if unsafe { GetShellWindow() }.is_invalid() {
        return false;
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let Ok(exe) = std::env::current_exe() else { return false };
    std::process::Command::new(format!("{windir}\\explorer.exe")).arg(exe).spawn().is_ok()
}

fn wait_for_resident(timeout: Duration) -> Option<HWND> {
    let start = Instant::now();
    loop {
        if let Some(h) = find_resident() {
            return Some(h);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn hand_off(hwnd: HWND, request: &Request) -> bool {
    let payload = encode(request);
    let cds = COPYDATASTRUCT {
        dwData: COPYDATA_REQUEST,
        cbData: (payload.len() * 2) as u32,
        lpData: payload.as_ptr() as *mut _,
    };
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut result),
        );
        sent.0 != 0 && result != 0
    }
}

fn hand_off_when_up(request: &Request) -> i32 {
    match wait_for_resident(Duration::from_secs(10)) {
        Some(hwnd) if hand_off(hwnd, request) => 0,
        _ => 1,
    }
}

/// Run the launch logic; returns the process exit code. A process that becomes
/// the resident returns only after the Window has closed.
pub fn run(args: &[String]) -> i32 {
    let resident = find_resident();
    match decide(args, resident.is_some()) {
        Decision::Register => crate::shell::register(),
        Decision::Unregister => crate::shell::unregister(),
        Decision::HandOff(request) => match resident {
            Some(hwnd) if hand_off(hwnd, &absolutize(request)) => 0,
            _ => 1,
        },
        Decision::BecomeResident(request) => {
            let request = absolutize(request);
            // Elevated and cold: the resident must not run elevated, so start
            // it through Explorer and hand the request over to it.
            if is_elevated() {
                if !relaunch_through_shell() {
                    return 1;
                }
                return hand_off_when_up(&request);
            }
            let Ok(mutex) = (unsafe { CreateMutexW(None, true, MUTEX) }) else { return 1 };
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                // Another launch won the race and is still creating its Window.
                unsafe {
                    let _ = CloseHandle(mutex);
                }
                return hand_off_when_up(&request);
            }
            // Never closed: the mutex lives as long as the process.
            let _ = mutex;
            crate::app::run(request)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(args: &[&str], running: bool) -> Decision {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        decide(&args, running)
    }

    fn open(paths: &[&str]) -> Request {
        Request::Open(paths.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn no_args_is_a_new_tab() {
        assert_eq!(d(&[], true), Decision::HandOff(Request::NewTab));
        assert_eq!(d(&[], false), Decision::BecomeResident(Request::NewTab));
    }

    #[test]
    fn one_path_opens_it() {
        assert_eq!(d(&["a.md"], true), Decision::HandOff(open(&["a.md"])));
        assert_eq!(d(&["a.md"], false), Decision::BecomeResident(open(&["a.md"])));
    }

    #[test]
    fn several_paths_open_in_order() {
        assert_eq!(
            d(&["a.md", "b.txt", "c"], true),
            Decision::HandOff(open(&["a.md", "b.txt", "c"]))
        );
        assert_eq!(
            d(&["a.md", "b.txt"], false),
            Decision::BecomeResident(open(&["a.md", "b.txt"]))
        );
    }

    #[test]
    fn register_and_unregister_ignore_the_resident() {
        for running in [false, true] {
            assert_eq!(d(&["--register"], running), Decision::Register);
            assert_eq!(d(&["--unregister"], running), Decision::Unregister);
        }
    }

    #[test]
    fn unknown_flags_and_empty_args_are_ignored() {
        assert_eq!(d(&[""], true), Decision::HandOff(Request::NewTab));
        assert_eq!(d(&["--bogus"], true), Decision::HandOff(Request::NewTab));
        assert_eq!(d(&["--bogus", "a.md"], false), Decision::BecomeResident(open(&["a.md"])));
    }

    #[test]
    fn payload_round_trips() {
        for r in [
            Request::NewTab,
            open(&["C:\\notes\\a.md"]),
            open(&["C:\\a b\\ü.txt", "D:\\x.md", "relative.md"]),
        ] {
            assert_eq!(decode(&encode(&r)), r);
        }
    }

    #[test]
    fn empty_payload_is_a_new_tab() {
        assert_eq!(decode(&[]), Request::NewTab);
    }
}
