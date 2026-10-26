//! Large files through a real, headless resident (SCRIVI-18): over 50 MB the
//! Tab's document is read-only with no undo and no wrap; at 50 MB it is a
//! normal editable file with wrap and undo. The resident writes the shown
//! document's settings to `SCRIVI_TEST_EDITOR` (debug builds).
//!
//! Its processes share a `SCRIVI_TEST_INSTANCE` of their own.

use std::collections::HashMap;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXE: &str = env!("CARGO_BIN_EXE_scrivi");
const MB: usize = 1024 * 1024;

fn instance() -> String {
    format!("{}-large", std::process::id())
}

fn resident_running() -> bool {
    unsafe { FindWindowW(&HSTRING::from(format!("Scrivi.Window.{}", instance())), PCWSTR::null()) }.is_ok()
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(60) {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

struct Resident(Child);
impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Run {
    dir: PathBuf,
}

impl Run {
    fn scrivi(&self, file: &Path) -> Command {
        let mut c = Command::new(EXE);
        c.arg(file)
            .env("SCRIVI_TEST_LOG", self.dir.join("requests.log"))
            .env("SCRIVI_TEST_TABS", self.dir.join("tabs.txt"))
            .env("SCRIVI_TEST_EDITOR", self.dir.join("editor.txt"))
            .env("SCRIVI_TEST_INSTANCE", instance())
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    /// The shown document's settings once `file`'s Tab is the one shown.
    fn editor_of(&self, file: &Path) -> HashMap<String, String> {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let mut found = HashMap::new();
        wait_until(&format!("{name} shown"), || {
            let text = std::fs::read_to_string(self.dir.join("editor.txt")).unwrap_or_default();
            let map: HashMap<String, String> =
                text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.into(), v.into())).collect();
            let shown = map.get("name") == Some(&name) && map.get("length").is_some_and(|l| l != "0");
            if shown {
                found = map;
            }
            shown
        });
        found
    }
}

fn write_text(path: &Path, bytes: usize) {
    let line = b"2026-10-26 12:00:00 INFO the quick brown fox jumps over the lazy dog\n";
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut written = 0;
    while written < bytes {
        let n = line.len().min(bytes - written);
        out.write_all(&line[..n]).unwrap();
        written += n;
    }
    out.flush().unwrap();
}

#[test]
fn a_file_over_50_mb_opens_read_only_without_undo_or_wrap_and_one_at_50_mb_does_not() {
    assert!(!resident_running());
    let dir = std::env::temp_dir().join(format!("scrivi-large-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };

    let small = dir.join("small.log");
    let exact = dir.join("exactly-50-mb.log");
    let large = dir.join("large.log");
    write_text(&small, 1000);
    write_text(&exact, 50 * MB);
    write_text(&large, 50 * MB + 1);

    let resident = Resident(run.scrivi(&small).spawn().unwrap());
    wait_until("the resident", resident_running);
    let editor = run.editor_of(&small);
    assert_eq!((editor["read_only"].as_str(), editor["undo"].as_str()), ("0", "1"));

    assert!(run.scrivi(&exact).status().unwrap().success());
    let editor = run.editor_of(&exact);
    assert_eq!(editor["read_only"], "0", "50 MB is not a Large file");
    assert_eq!(editor["undo"], "1");
    assert_ne!(editor["wrap"], "0", "50 MB wraps");

    assert!(run.scrivi(&large).status().unwrap().success());
    let editor = run.editor_of(&large);
    assert_eq!(editor["read_only"], "1");
    assert_eq!(editor["undo"], "0");
    assert_eq!(editor["wrap"], "0");
    assert_eq!(editor["length"], (50 * MB + 1).to_string());

    drop(resident);
    wait_until("the window to go away", || !resident_running());
    let _ = std::fs::remove_dir_all(&dir);
}
