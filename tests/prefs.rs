#![cfg(debug_assertions)]
//! Debug builds only: release ignores `SCRIVI_TEST_PREFS_DIR`, and this would
//! write the real scrivi.ini.
//!
//! Recent files survive a restart (SCRIVI-23): a real, headless resident with
//! its own `SCRIVI_TEST_PREFS_DIR`, so the real scrivi.ini and Recent
//! list are never touched. Opening a file writes scrivi.ini at once; a second
//! resident, started later on the same folder, keeps what the first wrote.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXE: &str = env!("CARGO_BIN_EXE_scrivi");

fn instance() -> String {
    format!("{}-prefs", std::process::id())
}

fn resident_up() -> bool {
    unsafe { FindWindowW(&HSTRING::from(format!("Scrivi.Window.{}", instance())), PCWSTR::null()) }.is_ok()
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
            .env("SCRIVI_TEST_INSTANCE", instance())
            .env("SCRIVI_TEST_PREFS_DIR", self.dir.join("prefs"))
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    /// The `[recent]` file names in scrivi.ini, newest first.
    fn recent(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.dir.join("prefs").join("scrivi.ini")).unwrap_or_default();
        let after = text.split("[recent]").nth(1).unwrap_or("");
        after
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(_, p)| Path::new(p).file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }
}

struct Resident(Child);
impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn recent_files_are_written_newest_first_and_survive_a_restart() {
    assert!(!resident_up());
    let dir = std::env::temp_dir().join(format!("scrivi-prefs-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };
    let files: Vec<PathBuf> = ["a.md", "b.txt", "c.md"]
        .iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, "x\n").unwrap();
            path
        })
        .collect();

    let first = Resident(run.scrivi(&[&files[0]]).spawn().unwrap());
    wait_until("the resident", resident_up);
    wait_until("a.md in the recent list", || run.recent() == ["a.md"]);
    assert!(run.scrivi(&[&files[1]]).status().unwrap().success());
    wait_until("b.txt on top", || run.recent() == ["b.txt", "a.md"]);
    // Opening an older one again moves it to the top.
    assert!(run.scrivi(&[&files[0]]).status().unwrap().success());
    wait_until("a.md back on top", || run.recent() == ["a.md", "b.txt"]);

    // A restart keeps the list.
    drop(first);
    wait_until("the window to go away", || !resident_up());
    let second = Resident(run.scrivi(&[&files[2]]).spawn().unwrap());
    wait_until("c.md on top of the old list", || run.recent() == ["c.md", "a.md", "b.txt"]);

    drop(second);
    wait_until("the window to go away", || !resident_up());
    let _ = std::fs::remove_dir_all(&dir);
}
