//! Hand-offs land in Tabs (spec §3, §14): a real, headless resident; later
//! launches with paths add Tabs or switch to one already open. The debug
//! resident writes its strip to `SCRIVI_TEST_TABS` after each hand-off.
//!
//! Its processes share a `SCRIVI_TEST_INSTANCE` of their own (debug builds
//! suffix the window class and mutex with it): no running scrivi interferes.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// This test's own resident.
fn instance() -> String {
    format!("{}-tabs", std::process::id())
}

const EXE: &str = env!("CARGO_BIN_EXE_scrivi");

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
            .env("SCRIVI_TEST_TABS", self.dir.join("tabs.txt"))
            .env("SCRIVI_TEST_INSTANCE", instance())
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    /// The strip as the resident last wrote it.
    fn strip(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("tabs.txt")).unwrap_or_default().lines().map(String::from).collect()
    }

    /// A later launch: it hands over and exits 0, and the strip becomes `want`.
    fn hand_off(&self, args: &[&Path], want: &[&str]) {
        assert!(self.scrivi(args).status().unwrap().success());
        wait_until(&format!("the strip {want:?}"), || self.strip() == want);
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

#[test]
fn launches_open_tabs_in_the_one_window() {
    assert!(!resident_up());
    let dir = std::env::temp_dir().join(format!("scrivi-tabs-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };
    let files: Vec<PathBuf> = ["a.md", "b.txt", "c.md", "d.log", "e.md"]
        .iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, format!("# {name}\n")).unwrap();
            path
        })
        .collect();

    // The first launch becomes the resident, with its file in the one Tab.
    let resident = Resident(run.scrivi(&[&files[0]]).spawn().unwrap());
    wait_until("the resident", resident_up);
    wait_until("the first Tab", || run.strip() == ["*a.md"]);

    // Opening five files from Explorer gives five Tabs; the newest is shown.
    for (i, f) in files.iter().enumerate().skip(1) {
        let mut want: Vec<String> = files[..=i].iter().map(|f| f.file_name().unwrap().to_string_lossy().into()).collect();
        want[i] = format!("*{}", want[i]);
        let want: Vec<&str> = want.iter().map(String::as_str).collect();
        run.hand_off(&[f], &want);
    }

    // Opening one of them again switches to its Tab.
    run.hand_off(&[&files[2]], &["a.md", "b.txt", "*c.md", "d.log", "e.md"]);

    // No arguments: a new Untitled tab.
    run.hand_off(&[], &["a.md", "b.txt", "c.md", "d.log", "e.md", "*Untitled 1"]);

    // A file that can't be read gets no Tab; the shown one stays.
    // (Hand-offs are handled in order, so the next one shows it left nothing behind.)
    run.hand_off(&[&dir.join("missing.md")], &["a.md", "b.txt", "c.md", "d.log", "e.md", "*Untitled 1"]);
    run.hand_off(&[&files[0]], &["*a.md", "b.txt", "c.md", "d.log", "e.md", "Untitled 1"]);

    drop(resident);
    wait_until("the window to go away", || !resident_up());
    let _ = std::fs::remove_dir_all(&dir);
}
