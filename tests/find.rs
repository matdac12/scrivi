//! The find bar through a real, headless resident (SCRIVI-20): Replace all is
//! one undo step, and Go to line lands in a 50 MB file. The commands arrive as
//! the menus' `WM_COMMAND`s, the inputs are the bar's own EDIT controls, and
//! the result is read back from the saved file.
//!
//! Its processes share a `SCRIVI_TEST_INSTANCE` of their own (debug builds
//! suffix the window class and mutex with it): no running scrivi interferes.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetDlgItem, GetWindowLongPtrW, GetWindowRect, PostMessageW, SendMessageW, GWL_STYLE,
    WM_CHAR, WM_COMMAND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_SETTEXT, WS_VISIBLE,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXE: &str = env!("CARGO_BIN_EXE_scrivi");

/// `app::cmd`: SAVE, UNDO, FIND, REPLACE, GO_TO_LINE.
const SAVE: usize = 105;
const UNDO: usize = 110;
const REPLACE: usize = 117;
const GO_TO_LINE: usize = 118;
/// The bar's EDIT controls' child ids, in `app::find`.
const ID_FIND: i32 = 0x1A1;
const ID_REPLACE: i32 = 0x1A2;
const ID_GOTO: i32 = 0x1A3;
const VK_RETURN: usize = 0x0D;

fn instance() -> String {
    format!("{}-find", std::process::id())
}

fn resident() -> Option<HWND> {
    unsafe { FindWindowW(&HSTRING::from(format!("Scrivi.Window.{}", instance())), PCWSTR::null()) }.ok()
}

fn wait_until(what: &str, limit: Duration, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while t.elapsed() < limit {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

const SHORT: Duration = Duration::from_secs(15);
const LONG: Duration = Duration::from_secs(120);

struct Run {
    dir: PathBuf,
}

impl Run {
    fn scrivi(&self, args: &[&Path]) -> Command {
        let mut c = Command::new(EXE);
        c.args(args)
            .env("SCRIVI_TEST_LOG", self.dir.join("requests.log"))
            .env("SCRIVI_TEST_TABS", self.dir.join("tabs.txt"))
            .env("SCRIVI_TEST_INSTANCE", instance())
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    fn strip(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("tabs.txt")).unwrap_or_default().lines().map(String::from).collect()
    }

    /// Hand `file` to the resident and wait until its Tab is the active one.
    fn show(&self, file: &Path) {
        assert!(self.scrivi(&[file]).status().unwrap().success());
        let want = format!("*{}", file.file_name().unwrap().to_string_lossy());
        wait_until(&format!("{want} active"), LONG, || self.strip().contains(&want));
    }
}

/// Kills the resident (by PID) however the test ends.
struct Resident(Child);
impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The file's bytes, once a save in flight lets it be read.
fn read_settled(file: &Path) -> Vec<u8> {
    let mut bytes = None;
    wait_until(&format!("{} to be readable", file.display()), SHORT, || {
        bytes = std::fs::read(file).ok();
        bytes.is_some()
    });
    bytes.unwrap()
}

fn command(hwnd: HWND, id: usize) {
    unsafe { PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(id), LPARAM(0)) }.unwrap();
}

/// A bar input, once it is shown (the Window itself stays hidden here).
fn input(hwnd: HWND, id: i32) -> HWND {
    let mut found = HWND::default();
    wait_until(&format!("input {id:#x}"), SHORT, || {
        found = unsafe { GetDlgItem(Some(hwnd), id) }.unwrap_or_default();
        !found.is_invalid() && unsafe { GetWindowLongPtrW(found, GWL_STYLE) } as u32 & WS_VISIBLE.0 != 0
    });
    found
}

fn type_into(edit: HWND, text: &str) {
    let z = HSTRING::from(text);
    unsafe { SendMessageW(edit, WM_SETTEXT, None, Some(LPARAM(z.as_ptr() as isize))) };
}

/// Send `commands`, then wait until `file` was written again.
fn save_and_wait(hwnd: HWND, file: &Path, limit: Duration) {
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    // A save in flight (ReplaceFileW) leaves the path missing or locked for a moment.
    wait_until("the file to be writable", SHORT, || {
        std::fs::File::options().write(true).open(file).and_then(|f| f.set_modified(long_ago)).is_ok()
    });
    command(hwnd, SAVE);
    wait_until(&format!("{} saved", file.display()), limit, || {
        std::fs::metadata(file).and_then(|m| m.modified()).is_ok_and(|t| t != long_ago)
    });
}

#[test]
fn replace_all_is_one_undo_step_and_go_to_line_works_in_a_50_mb_file() {
    assert!(resident().is_none());
    let dir = std::env::temp_dir().join(format!("scrivi-find-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };

    let notes = dir.join("notes.txt");
    let original = b"foo bar foo\nFoo foo\nnothing here\n".to_vec();
    std::fs::write(&notes, &original).unwrap();
    // Exactly 50 MB is not a Large file: 1,638,400 lines of 32 bytes.
    let big = dir.join("big.txt");
    let lines = 50 * 1024 * 1024 / 32;
    let mut text = String::with_capacity(50 * 1024 * 1024);
    for n in 1..=lines {
        text.push_str(&format!("line {n:07} ..................\n"));
    }
    assert_eq!(text.len(), 50 * 1024 * 1024);
    std::fs::write(&big, &text).unwrap();

    let resident_process = Resident(run.scrivi(&[&notes]).spawn().unwrap());
    wait_until("the resident", SHORT, || resident().is_some());
    let hwnd = resident().unwrap();
    run.show(&notes);

    // Ctrl+H, "foo" → "x", [Replace all]: 4 matches, whatever their case.
    command(hwnd, REPLACE);
    type_into(input(hwnd, ID_FIND), "foo");
    let replace = input(hwnd, ID_REPLACE);
    type_into(replace, "x");
    // The button sits 8 + 8 + 72 + 8 DIPs right of the replace input; click its middle.
    let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let mut r = RECT::default();
    unsafe { GetWindowRect(replace, &mut r) }.unwrap();
    let mut at = POINT { x: r.right + (96.0 * scale + 30.0 * scale) as i32, y: (r.top + r.bottom) / 2 };
    unsafe { ScreenToClient(hwnd, &mut at) }.ok().unwrap();
    let lparam = LPARAM((at.x as isize & 0xffff) | ((at.y as isize & 0xffff) << 16));
    unsafe { PostMessageW(Some(hwnd), WM_LBUTTONDOWN, WPARAM(1), lparam) }.unwrap();
    // Wait for the replacement to be in the document: save until it shows.
    wait_until("Replace all", SHORT, || {
        save_and_wait(hwnd, &notes, SHORT);
        std::fs::read(&notes).is_ok_and(|b| b == b"x bar x\nx x\nnothing here\n")
    });

    // One undo puts all four back.
    command(hwnd, UNDO);
    save_and_wait(hwnd, &notes, SHORT);
    assert_eq!(read_settled(&notes), original);

    // Ctrl+G 1000000 in the 50 MB file: the caret is at the start of that
    // line, so a typed X goes there.
    run.show(&big);
    command(hwnd, GO_TO_LINE);
    let goto = input(hwnd, ID_GOTO);
    type_into(goto, "1000000");
    unsafe { SendMessageW(goto, WM_KEYDOWN, Some(WPARAM(VK_RETURN)), Some(LPARAM(0))) };
    let editor = unsafe { FindWindowExW(Some(hwnd), None, &HSTRING::from("Scintilla"), PCWSTR::null()) }.unwrap();
    unsafe { SendMessageW(editor, WM_CHAR, Some(WPARAM('X' as usize)), Some(LPARAM(0))) };
    save_and_wait(hwnd, &big, LONG);
    let saved = read_settled(&big);
    let at = 999_999 * 32;
    assert_eq!(&saved[at..at + 6], b"Xline ", "line 1,000,000 starts at byte {at}");
    assert_eq!(saved.len(), 50 * 1024 * 1024 + 1);
    assert_eq!(&saved[..5], b"line ");

    drop(resident_process);
    wait_until("the window to go away", SHORT, || resident().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
