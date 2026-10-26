//! Launch decisions: one resident, every later launch hands over (spec §3).
//! `decide` is pure; the adapter below it owns the mutex, `FindWindow`,
//! `WM_COPYDATA` and the relaunch through `explorer.exe` when elevated. That
//! relaunch happens once: with UAC off the Explorer child is elevated too, and
//! must run rather than relaunch again.

use std::ffi::OsString;
use std::io::Write;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetShellWindow, GetWindowThreadProcessId,
    SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA,
};

use crate::proto::{self, COPYDATA_REQUEST};

/// What a launch asks the Window to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Request {
    /// An Untitled tab.
    NewTab,
    /// Open each path in its own Tab, or switch to its Tab if already open.
    Open(Vec<PathBuf>),
}

/// The facts a launch decides on: what `find_resident` and the process itself
/// say, never a guess.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    /// A resident has its Window up.
    pub resident: bool,
    /// This process runs elevated.
    pub elevated: bool,
    /// Explorer started this process: the relaunch through Explorer has already
    /// happened (or the user double-clicked the exe).
    pub from_shell: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Decision {
    Register,
    Unregister,
    /// `-h` / `--help`: print the usage, exit 0.
    Help,
    /// An unknown `--flag`: print the usage, exit 2.
    BadUsage,
    HandOff(Request),
    /// Elevated and cold, and not started by Explorer: relaunch unelevated
    /// through Explorer, then hand the request over to that child.
    Relaunch(Request),
    BecomeResident(Request),
}

/// The usage line for `--help` and for a flag scrivi does not know.
const USAGE: &str = "\
usage: scrivi [--] [file]...
       scrivi --register | --unregister | --help";

/// What this launch asks for. `--` ends the flags: every later argument is a
/// path, so a file named `--notes.md` still opens. Anything else starting with
/// `--` is a flag scrivi does not know, and saying so beats ignoring it.
pub fn decide(args: &[String], state: State) -> Decision {
    let mut paths = Vec::new();
    let mut flags = true;
    for a in args {
        let s = a.as_str();
        if s.is_empty() {
            continue;
        }
        if flags {
            match s {
                "--" => {
                    flags = false;
                    continue;
                }
                "--register" => return Decision::Register,
                "--unregister" => return Decision::Unregister,
                "-h" | "--help" => return Decision::Help,
                _ if s.starts_with("--") => return Decision::BadUsage,
                _ => {}
            }
        }
        paths.push(PathBuf::from(s));
    }
    let request = if paths.is_empty() {
        Request::NewTab
    } else {
        Request::Open(paths)
    };
    if state.resident {
        Decision::HandOff(request)
    } else if state.elevated && !state.from_shell {
        Decision::Relaunch(request)
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
            paths
                .into_iter()
                .map(|p| std::path::absolute(&p).unwrap_or(p))
                .collect(),
        ),
    }
}

fn find_resident() -> Option<HWND> {
    unsafe { FindWindowW(proto::class(), PCWSTR::null()) }.ok()
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
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new(format!("{windir}\\explorer.exe"))
        .arg(exe)
        .spawn()
        .is_ok()
}

/// Whether Explorer started this process. With UAC off, or as the built-in
/// Administrator, the child Explorer starts is elevated too: it must run, not
/// relaunch again and loop. A double-click comes the same way, so it runs too.
fn started_by_shell() -> bool {
    parent_image_name(std::process::id()).is_some_and(|name| is_shell_image(&name))
}

fn is_shell_image(name: &str) -> bool {
    name.eq_ignore_ascii_case("explorer.exe")
}

/// The image name (no directory) of `pid`'s parent, e.g. `explorer.exe`.
fn parent_image_name(pid: u32) -> Option<String> {
    let (parent, _) = process_entry(pid)?;
    process_entry(parent).map(|(_, name)| name)
}

/// `pid`'s parent process id, and its own image name.
fn process_entry(pid: u32) -> Option<(u32, String)> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    found = Some((
                        entry.th32ParentProcessID,
                        String::from_utf16_lossy(&entry.szExeFile[..end]),
                    ));
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
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

/// One retry: a resident busy with a dialog, or a slow open, can miss the
/// first message, and `SMTO_ABORTIFHUNG` gives up on it.
fn retry_once(mut attempt: impl FnMut() -> bool) -> bool {
    attempt() || {
        std::thread::sleep(Duration::from_millis(250));
        attempt()
    }
}

fn hand_off_with_retry(hwnd: HWND, request: &Request) -> bool {
    retry_once(|| hand_off(hwnd, request))
}

fn hand_off_when_up(request: &Request) -> i32 {
    match wait_for_resident(Duration::from_secs(10)) {
        Some(hwnd) if hand_off_with_retry(hwnd, request) => 0,
        _ => 1,
    }
}

/// Run the launch logic; returns the process exit code. A process that becomes
/// the resident returns only after the Window has closed.
pub fn run(args: &[String]) -> i32 {
    let resident = find_resident();
    let elevated = is_elevated();
    let state = State {
        resident: resident.is_some(),
        elevated,
        // Only the elevated path relaunches, and only it needs to know.
        from_shell: elevated && started_by_shell(),
    };
    match decide(args, state) {
        Decision::Register => crate::shell::register(),
        Decision::Unregister => crate::shell::unregister(),
        Decision::Help => {
            let _ = writeln!(std::io::stdout(), "{USAGE}");
            0
        }
        Decision::BadUsage => {
            let _ = writeln!(std::io::stderr(), "{USAGE}");
            2
        }
        Decision::HandOff(request) => match resident {
            Some(hwnd) if hand_off_with_retry(hwnd, &absolutize(request)) => 0,
            _ => 1,
        },
        Decision::Relaunch(request) => {
            // Its paths are relative to this process's directory, not Explorer's.
            let request = absolutize(request);
            if !relaunch_through_shell() {
                return 1;
            }
            hand_off_when_up(&request)
        }
        Decision::BecomeResident(request) => {
            let request = absolutize(request);
            let Ok(mutex) = (unsafe { CreateMutexW(None, true, proto::mutex()) }) else {
                return 1;
            };
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

    fn d(args: &[&str], state: State) -> Decision {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        decide(&args, state)
    }

    fn running() -> State {
        State {
            resident: true,
            ..State::default()
        }
    }

    fn cold() -> State {
        State::default()
    }

    fn open(paths: &[&str]) -> Request {
        Request::Open(paths.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn no_args_is_a_new_tab() {
        assert_eq!(d(&[], running()), Decision::HandOff(Request::NewTab));
        assert_eq!(d(&[], cold()), Decision::BecomeResident(Request::NewTab));
    }

    #[test]
    fn one_path_opens_it() {
        assert_eq!(d(&["a.md"], running()), Decision::HandOff(open(&["a.md"])));
        assert_eq!(
            d(&["a.md"], cold()),
            Decision::BecomeResident(open(&["a.md"]))
        );
    }

    #[test]
    fn several_paths_open_in_order() {
        assert_eq!(
            d(&["a.md", "b.txt", "c"], running()),
            Decision::HandOff(open(&["a.md", "b.txt", "c"]))
        );
        assert_eq!(
            d(&["a.md", "b.txt"], cold()),
            Decision::BecomeResident(open(&["a.md", "b.txt"]))
        );
    }

    #[test]
    fn register_and_unregister_ignore_the_resident() {
        for state in [
            cold(),
            running(),
            State {
                elevated: true,
                from_shell: true,
                ..running()
            },
        ] {
            assert_eq!(d(&["--register"], state), Decision::Register);
            assert_eq!(d(&["--unregister"], state), Decision::Unregister);
        }
    }

    #[test]
    fn empty_args_are_ignored() {
        assert_eq!(d(&[""], running()), Decision::HandOff(Request::NewTab));
        assert_eq!(
            d(&["", "a.md"], cold()),
            Decision::BecomeResident(open(&["a.md"]))
        );
    }

    #[test]
    fn an_unknown_flag_is_a_usage_error() {
        for args in [
            &["--bogus"][..],
            &["--notes.md"][..],
            &["a.md", "--bogus"][..],
            &["--bogus", "a.md"][..],
        ] {
            assert_eq!(d(args, cold()), Decision::BadUsage, "{args:?}");
            assert_eq!(d(args, running()), Decision::BadUsage, "{args:?}");
        }
    }

    #[test]
    fn a_double_dash_ends_the_flags_so_the_rest_are_paths() {
        assert_eq!(
            d(&["--", "--notes.md"], cold()),
            Decision::BecomeResident(open(&["--notes.md"]))
        );
        assert_eq!(
            d(&["a.md", "--", "-h"], cold()),
            Decision::BecomeResident(open(&["a.md", "-h"]))
        );
        assert_eq!(
            d(&["--"], cold()),
            Decision::BecomeResident(Request::NewTab)
        );
        assert_eq!(
            d(&["--", "a.md"], running()),
            Decision::HandOff(open(&["a.md"]))
        );
    }

    #[test]
    fn help_flags_ask_for_the_usage() {
        for flag in ["-h", "--help"] {
            assert_eq!(d(&[flag], cold()), Decision::Help);
            assert_eq!(d(&[flag], running()), Decision::Help);
            assert_eq!(d(&["a.md", flag], cold()), Decision::Help);
        }
    }

    #[test]
    fn an_elevated_launch_started_by_explorer_runs_rather_than_relaunching() {
        // UAC off, or the built-in Administrator: the Explorer child is
        // elevated too, and relaunching it again would loop forever.
        let shell = State {
            elevated: true,
            from_shell: true,
            ..State::default()
        };
        assert_eq!(
            d(&["a.md"], shell),
            Decision::BecomeResident(open(&["a.md"]))
        );
    }

    #[test]
    fn an_elevated_cold_launch_relaunches_through_explorer() {
        let elevated = State {
            elevated: true,
            ..State::default()
        };
        assert_eq!(d(&["a.md"], elevated), Decision::Relaunch(open(&["a.md"])));
    }

    #[test]
    fn a_running_resident_still_gets_the_request_from_an_elevated_launch() {
        let elevated = State {
            elevated: true,
            ..running()
        };
        assert_eq!(d(&[], elevated), Decision::HandOff(Request::NewTab));
    }

    #[test]
    fn a_hand_off_that_fails_is_attempted_once_more() {
        let mut tries = 0;
        assert!(retry_once(|| {
            tries += 1;
            tries == 2
        }));
        assert_eq!(tries, 2);
    }

    #[test]
    fn a_hand_off_that_fails_twice_gives_up() {
        let mut tries = 0;
        assert!(!retry_once(|| {
            tries += 1;
            false
        }));
        assert_eq!(tries, 2);
    }

    #[test]
    fn the_shell_is_recognised_by_its_image_name() {
        assert!(is_shell_image("explorer.exe"));
        assert!(is_shell_image("EXPLORER.EXE"));
        assert!(!is_shell_image("cmd.exe"));
        assert!(!is_shell_image("explorer.exe.bak"));
    }

    #[test]
    fn a_process_can_be_named_by_its_parent_id() {
        // Not Explorer under cargo test, but it must name *something*.
        let name = parent_image_name(std::process::id());
        assert!(name.as_ref().is_some_and(|n| !n.is_empty()), "{name:?}");
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
