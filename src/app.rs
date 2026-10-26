//! The Window, its wndproc, the message loop and the glue between modules.
//!
//! Placeholder resident until 06 builds the real Window: a hidden window of
//! class `Scrivi.Window` that appends each request it receives to the file named
//! by `SCRIVI_TEST_LOG`, if set. 06 keeps `run` and `on_handoff` as the seam.

#[cfg(debug_assertions)]
use std::io::Write;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::launch::{self, Request};
use crate::proto::{CLASS, COPYDATA_REQUEST};

/// Create the Window, apply `initial`, and pump messages until it closes.
/// Returns the process exit code.
pub fn run(initial: Request) -> i32 {
    unsafe {
        let Ok(hinstance) = GetModuleHandleW(None) else { return 1 };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return 1;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            PCWSTR::null(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinstance.into()),
            None,
        ) else {
            return 1;
        };
        // An elevated launch hands over from a higher integrity level.
        let _ = ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None);
        on_handoff(initial);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}

/// A request from this launch (`run`'s `initial`) or a later one (`WM_COPYDATA`).
pub fn on_handoff(request: Request) {
    // Test hook, debug builds only.
    #[cfg(not(debug_assertions))]
    let _ = &request;
    #[cfg(debug_assertions)]
    log_request(request);
}

#[cfg(debug_assertions)]
fn log_request(request: Request) {
    let Some(path) = std::env::var_os("SCRIVI_TEST_LOG") else { return };
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let line = match request {
        Request::NewTab => "NewTab".to_string(),
        Request::Open(paths) => {
            let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
            format!("Open\t{}", list.join("\t"))
        }
    };
    let _ = writeln!(file, "{line}");
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COPYDATA => {
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData != COPYDATA_REQUEST {
                    return LRESULT(0);
                }
                let units = cds.cbData as usize / 2;
                let payload: &[u16] = if units == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(cds.lpData as *const u16, units)
                };
                on_handoff(launch::decode(payload));
                let _ = SetForegroundWindow(hwnd);
                LRESULT(1)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
