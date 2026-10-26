//! End-to-end launch tests against the real binary, headless: the placeholder
//! resident has a hidden window and logs each request to `SCRIVI_TEST_LOG`.
//!
//! Each test gives every process it starts its own `SCRIVI_TEST_INSTANCE`
//! (debug builds suffix the window class and mutex with it), so it never
//! meets a running scrivi or another worktree's test.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXE: &str = env!("CARGO_BIN_EXE_scrivi");

/// One test's own resident: its `SCRIVI_TEST_INSTANCE` value.
struct Instance(String);

impl Instance {
    fn new(test: &str) -> Instance {
        Instance(format!("{}-{test}", std::process::id()))
    }

    fn class(&self) -> HSTRING {
        HSTRING::from(format!("Scrivi.Window.{}", self.0))
    }

    fn find(&self) -> Option<HWND> {
        unsafe { FindWindowW(&self.class(), PCWSTR::null()) }.ok()
    }

    /// A second Window of this instance after `hwnd`, if any.
    fn another(&self, hwnd: HWND) -> bool {
        unsafe { FindWindowExW(None, Some(hwnd), &self.class(), PCWSTR::null()) }.is_ok()
    }

    fn scrivi(&self, log: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(EXE);
        c.args(args)
            .env("SCRIVI_TEST_INSTANCE", &self.0)
            .env("SCRIVI_TEST_LOG", log)
            .env("SCRIVI_TEST_ALLOW_ELEVATED", "1")
            .creation_flags(CREATE_NO_WINDOW);
        c
    }
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

fn wait_exit(child: &mut Child, what: &str) -> ExitStatus {
    let t = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what} to exit"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// A handing-off process exits once the resident has *posted* the request, so the log line
/// lands a moment later: poll until the log holds at least `n` lines (5 s), then return them.
fn wait_lines(log: &Path, n: usize) -> Vec<String> {
    let t = Instant::now();
    loop {
        let l = lines(log);
        if l.len() >= n {
            return l;
        }
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "timed out waiting for {n} request lines, have {l:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Kills the resident (by PID) if the test fails half-way, so none is left behind.
struct Resident(Child);
impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn temp_dir(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("scrivi-launch-test-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(debug_assertions)]
fn key_exists(subkey: &str) -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    let k: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut h = HKEY::default();
        let ok = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            None,
            KEY_READ,
            &mut h,
        )
        .is_ok();
        if ok {
            let _ = RegCloseKey(h);
        }
        ok
    }
}

#[cfg(debug_assertions)]
fn delete_tree(subkey: &str) {
    use windows::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW, HKEY_CURRENT_USER};
    let k: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
    }
}

#[test]
fn launch_lifecycle() {
    let me = Instance::new("lifecycle");
    let (find, scrivi) = (
        || me.find(),
        |log: &Path, args: &[&str]| me.scrivi(log, args),
    );
    assert!(Path::new(EXE).exists());
    assert!(find().is_none());
    let dir = temp_dir("lifecycle");
    let log = dir.join("requests.log");

    // --register / --unregister with no resident: exit 0, nothing started.
    // Only ever under a throwaway prefix (debug builds); the real registry is never touched.
    #[cfg(debug_assertions)]
    {
        let prefix = format!(
            r"Software\ScriviTest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        );
        let prog = format!(r"{prefix}\Software\Classes\Scrivi.TextFile");
        let run = |arg: &str| {
            let mut c = scrivi(&log, &[arg]);
            c.env("SCRIVI_TEST_REGISTRY_PREFIX", &prefix);
            c.status().unwrap().success()
        };
        let registered = run("--register") && key_exists(&prog);
        let removed = run("--unregister") && !key_exists(&prog);
        delete_tree(&prefix);
        assert!(
            registered,
            "--register did not write the ProgID under the prefix"
        );
        assert!(removed, "--unregister left the ProgID under the prefix");
    }
    assert!(find().is_none());

    // A first launch with a path becomes the resident and applies the path.
    let first = dir.join("first.md");
    let mut resident = Resident(scrivi(&log, &[first.to_str().unwrap()]).spawn().unwrap());
    wait_until("the Scrivi.Window window", || find().is_some());
    let hwnd = find().unwrap();
    assert_eq!(wait_lines(&log, 1)[0], format!("Open\t{}", first.display()));

    // A second launch with a path hands over and exits 0; no second window.
    let notes = dir.join("notes.md");
    let status = scrivi(&log, &[notes.to_str().unwrap()]).status().unwrap();
    assert!(status.success());
    assert_eq!(wait_lines(&log, 2)[1], format!("Open\t{}", notes.display()));
    assert_eq!(find(), Some(hwnd));
    assert!(!me.another(hwnd));
    assert!(resident.0.try_wait().unwrap().is_none(), "resident died");

    // No arguments: a new tab. Several paths: all of them, in order.
    assert!(scrivi(&log, &[]).status().unwrap().success());
    assert_eq!(wait_lines(&log, 3)[2], "NewTab");
    let (a, b) = (dir.join("a.txt"), dir.join("b with space.md"));
    assert!(scrivi(&log, &[a.to_str().unwrap(), b.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    assert_eq!(
        wait_lines(&log, 4)[3],
        format!("Open\t{}\t{}", a.display(), b.display())
    );

    // A relative path arrives absolute.
    let mut rel = scrivi(&log, &["rel.md"]);
    rel.current_dir(&dir);
    assert!(rel.status().unwrap().success());
    assert_eq!(
        wait_lines(&log, 5)[4],
        format!("Open\t{}", dir.join("rel.md").display())
    );
    let before = wait_lines(&log, 5).len();

    // Explorer opening 10 files at once leaves one process: ten concurrent
    // launches all hand over and exit 0, and the one resident saw all ten.
    let files: Vec<String> = (0..10)
        .map(|i| dir.join(format!("f{i}.md")).display().to_string())
        .collect();
    let mut children: Vec<Child> = files
        .iter()
        .map(|f| scrivi(&log, &[f.as_str()]).spawn().unwrap())
        .collect();
    for c in &mut children {
        assert!(wait_exit(c, "a concurrent launch").success());
    }
    let mut got: Vec<String> = wait_lines(&log, before + files.len())[before..].to_vec();
    got.sort();
    let mut want: Vec<String> = files.iter().map(|f| format!("Open\t{f}")).collect();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(find(), Some(hwnd));
    assert!(!me.another(hwnd));
    assert!(resident.0.try_wait().unwrap().is_none(), "resident died");

    // Killing the resident frees the class.
    drop(resident);
    wait_until("the window to go away", || find().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The command line: an unknown `--flag` is refused with the usage and asks no
/// Window; `--help` prints the usage; `--` makes the rest a path.
#[test]
fn a_bad_flag_is_refused_and_a_double_dash_opens_the_path_after_it() {
    let me = Instance::new("args");
    let dir = temp_dir("args");
    let log = dir.join("requests.log");
    assert!(me.find().is_none());

    // Unknown flags: exit 2, the usage on stderr, nothing opened.
    for args in [&["--bogus"][..], &["--notes.md"][..]] {
        let out = me.scrivi(&log, args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("usage:"), "{args:?}: {err:?}");
    }
    assert!(lines(&log).is_empty(), "{:?}", lines(&log));
    assert!(me.find().is_none());

    // --help prints the same usage and exits 0.
    for flag in ["--help", "-h"] {
        let out = me.scrivi(&log, &[flag]).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{flag}");
        let out_text = String::from_utf8_lossy(&out.stdout);
        assert!(out_text.contains("usage:"), "{flag}: {out_text:?}");
    }
    assert!(me.find().is_none());

    // A resident, then `-- <a path starting with -->`: it opens, and takes the
    // lone empty Untitled Tab over instead of opening beside it.
    let tabs_log = dir.join("tabs.log");
    let mut start = me.scrivi(&log, &[]);
    start.env("SCRIVI_TEST_TABS", &tabs_log);
    let resident = Resident(start.spawn().unwrap());
    wait_until("the Scrivi.Window window", || me.find().is_some());
    assert_eq!(wait_lines(&log, 1), vec!["NewTab".to_string()]);
    let notes = dir.join("--notes.md");
    std::fs::write(&notes, "a file whose name looks like a flag\n").unwrap();
    assert!(me
        .scrivi(&log, &["--", notes.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    assert_eq!(wait_lines(&log, 2)[1], format!("Open\t{}", notes.display()));
    let strip = "*--notes.md\n".to_string();
    wait_until("the strip to hold only the opened file", || {
        std::fs::read_to_string(&tabs_log).is_ok_and(|t| t == strip)
    });
    drop(resident);
    wait_until("the window to go away", || me.find().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two cold launches at once: one becomes the resident, the other hands over to it.
#[test]
fn cold_race_leaves_one_resident() {
    let me = Instance::new("race");
    let (find, scrivi) = (
        || me.find(),
        |log: &Path, args: &[&str]| me.scrivi(log, args),
    );
    assert!(find().is_none());
    let dir = temp_dir("race");
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("requests.log");

    let files: Vec<String> = (0..4)
        .map(|i| dir.join(format!("r{i}.md")).display().to_string())
        .collect();
    let mut children: Vec<Child> = files
        .iter()
        .map(|f| scrivi(&log, &[f.as_str()]).spawn().unwrap())
        .collect();
    wait_until("the resident window", || find().is_some());
    wait_lines(&log, files.len());
    // Exactly one of the four is still running: the resident.
    let mut resident = None;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        let alive: Vec<usize> = (0..children.len())
            .filter(|&i| children[i].try_wait().unwrap().is_none())
            .collect();
        if alive.len() == 1 {
            resident = Some(alive[0]);
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let resident = resident.expect("exactly one process should remain");
    let resident = Resident(children.remove(resident));
    for c in &mut children {
        assert!(wait_exit(c, "a racing launch").success());
    }
    let mut got = lines(&log);
    got.sort();
    let mut want: Vec<String> = files.iter().map(|f| format!("Open\t{f}")).collect();
    want.sort();
    assert_eq!(got, want);
    drop(resident);
    wait_until("the window to go away", || find().is_none());
}
