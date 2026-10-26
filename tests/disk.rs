//! Disk changes through a real, headless resident (SCRIVI-19): a file
//! rewritten from outside reloads within about a second, scrivi's own atomic
//! save never counts as a Disk change, and a deleted file keeps its text
//! with the deleted-on-disk Notice until Save writes it again. The resident
//! writes the shown document's state to `SCRIVI_TEST_EDITOR` (debug builds).
//!
//! Its processes share a `SCRIVI_TEST_INSTANCE` of their own.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_COMMAND};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXE: &str = env!("CARGO_BIN_EXE_scrivi");
/// `app::cmd::SAVE`.
const SAVE: usize = 105;

fn instance() -> String {
    format!("{}-disk", std::process::id())
}

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
            .env("SCRIVI_TEST_EDITOR", self.dir.join("editor.txt"))
            .env("SCRIVI_TEST_INSTANCE", instance())
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }

    fn editor(&self) -> HashMap<String, String> {
        let text = std::fs::read_to_string(self.dir.join("editor.txt")).unwrap_or_default();
        text.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.into(), v.into()))
            .collect()
    }

    fn field(&self, key: &str) -> String {
        self.editor().remove(key).unwrap_or_default()
    }
}

fn save() {
    unsafe {
        PostMessageW(
            Some(resident().expect("the resident")),
            WM_COMMAND,
            WPARAM(SAVE),
            LPARAM(0),
        )
    }
    .unwrap();
}

#[test]
fn a_rewrite_reloads_a_save_is_no_disk_change_and_a_deleted_file_keeps_its_text() {
    assert!(resident().is_none());
    let dir = std::env::temp_dir().join(format!("scrivi-disk-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = Run { dir: dir.clone() };
    let file = dir.join("notes.md");
    std::fs::write(&file, "# one\n").unwrap();

    let _resident = Resident(run.scrivi(&file).spawn().unwrap());
    wait_until("the file shown", || {
        run.field("name") == "notes.md" && run.field("length") == "6"
    });
    let loads = |run: &Run| run.field("loads").parse::<usize>().unwrap();

    // Rewritten from outside (what PowerShell's Set-Content does): reloads on its own.
    let first_loads = loads(&run);
    let t = Instant::now();
    std::fs::write(&file, "# two, and longer\n").unwrap();
    wait_until("the reload", || run.field("length") == "18");
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "reloaded after {:?}",
        t.elapsed()
    );
    assert_eq!(run.field("notice"), "none");
    assert_eq!(loads(&run), first_loads + 1);

    // Ctrl+S: an atomic replace that the folder watcher sees as renames and
    // deletes. No reload, no Notice, no unsaved dot.
    let before_save = loads(&run);
    save();
    std::thread::sleep(Duration::from_millis(3000));
    assert_eq!(loads(&run), before_save, "the own save reloaded the Tab");
    assert_eq!(
        (run.field("notice").as_str(), run.field("unsaved").as_str()),
        ("none", "false")
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "# two, and longer\n"
    );

    // Deleted: the text stays, marked unsaved, with the Notice.
    std::fs::remove_file(&file).unwrap();
    wait_until("the deleted-on-disk Notice", || {
        run.field("notice") == "DeletedOnDisk"
    });
    assert_eq!(
        (run.field("unsaved").as_str(), run.field("length").as_str()),
        ("true", "18")
    );

    // Save writes the file again at its old path, and the Notice goes.
    save();
    wait_until("the file written again", || file.exists());
    wait_until("the Notice gone", || run.field("notice") == "none");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "# two, and longer\n"
    );
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(
        (run.field("notice").as_str(), run.field("unsaved").as_str()),
        ("none", "false")
    );

    drop(_resident);
    wait_until("the window to go away", || resident().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
