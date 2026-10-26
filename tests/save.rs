//! Open and save through a real, headless resident (SCRIVI-16): a file saved
//! without an edit comes back byte for byte, and File › Line endings
//! converts the Tab before it is saved. The commands arrive as the menus'
//! `WM_COMMAND`s, as if picked from File.
//!
//! Its processes share a `SCRIVI_TEST_INSTANCE` of their own (debug builds
//! suffix the window class and mutex with it): no running scrivi interferes.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_COMMAND};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// This test's own resident.
fn instance() -> String {
    format!("{}-save", std::process::id())
}

const EXE: &str = env!("CARGO_BIN_EXE_scrivi");
/// `app::cmd::SAVE` and `app::cmd::EOL` (+ 0 for LF).
const SAVE: usize = 105;
const EOL_LF: usize = 130;

fn resident() -> Option<HWND> {
    unsafe {
        FindWindowW(
            &HSTRING::from(format!("Scrivi.Window.{}", instance())),
            PCWSTR::null(),
        )
    }
    .ok()
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

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
        std::fs::read_to_string(self.dir.join("tabs.txt"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// Hand `file` to the resident and wait until its Tab is the active one.
    fn show(&self, file: &Path) {
        assert!(self.scrivi(&[file]).status().unwrap().success());
        let want = format!("*{}", file.file_name().unwrap().to_string_lossy());
        wait_until(&format!("{want} active"), || self.strip().contains(&want));
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

/// Send menu commands to the Window, then wait until `file` was written again.
fn command_and_wait_for_save(file: &Path, commands: &[usize]) {
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    let hwnd = resident().expect("the resident");
    for id in commands {
        unsafe { PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(*id), LPARAM(0)) }.unwrap();
    }
    wait_until(&format!("{} saved", file.display()), || {
        std::fs::metadata(file)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t != long_ago)
    });
}

#[test]
fn saving_writes_unedited_files_back_byte_for_byte_and_converts_line_endings() {
    assert!(resident().is_none());
    let dir = std::env::temp_dir().join(format!("scrivi-save-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };

    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(
        "Perché è già così\r\n😀 fine\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    let files: Vec<(PathBuf, Vec<u8>)> = [
        ("utf16le.txt", utf16),
        (
            "ansi.txt",
            b"Perch\xe9 la citt\xe0 \xe8 gi\xe0 cos\xec: 20 \x80\nfine\n".to_vec(),
        ),
        (
            "crlf.md",
            b"# Title\r\n\r\n- one\r\n- two\r\nno final break".to_vec(),
        ),
    ]
    .into_iter()
    .map(|(name, bytes)| {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        (path, bytes)
    })
    .collect();

    let resident_process = Resident(run.scrivi(&[&files[0].0]).spawn().unwrap());
    wait_until("the resident", || resident().is_some());

    // File › Save on each, unedited: the same bytes.
    for (path, bytes) in &files {
        run.show(path);
        command_and_wait_for_save(path, &[SAVE]);
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }

    // File › Line endings › LF, then Save: the CRLF file is LF now.
    let crlf = &files[2].0;
    run.show(crlf);
    command_and_wait_for_save(crlf, &[EOL_LF, SAVE]);
    assert_eq!(
        std::fs::read(crlf).unwrap(),
        b"# Title\n\n- one\n- two\nno final break"
    );

    // No temp file is left next to the saved ones.
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "ansi.txt",
            "crlf.md",
            "requests.log",
            "tabs.txt",
            "utf16le.txt"
        ]
    );

    drop(resident_process);
    wait_until("the window to go away", || resident().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
