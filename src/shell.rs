//! Windows integration: registry key list for --register / --unregister, jump list (spec §12).
//!
//! `registry_plan` is pure. `apply` and `remove` write it under HKCU, below a
//! caller-given prefix (empty for the real registry, a throwaway subtree in
//! tests). Nothing here touches `UserChoice`, a hash, or HKLM.

use std::path::Path;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegOpenKeyExW, RegQueryInfoKeyW,
    RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, ShellExecuteW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const PROG_ID: &str = "Scrivi.TextFile";
const APP_NAME: &str = "Scrivi";
const CAPABILITIES: &str = r"Software\Scrivi\Capabilities";
const REGISTERED_APPS: &str = r"Software\RegisteredApplications";

/// Extensions scrivi registers for (§12). `.` is the extensionless pseudo-type.
pub const EXTENSIONS: &[&str] = &[
    ".txt",
    ".md",
    ".markdown",
    ".log",
    ".ini",
    ".json",
    ".csv",
    ".env",
    ".cfg",
    ".conf",
    ".yaml",
    ".yml",
    ".toml",
    ".xml",
    ".",
];

/// One registry value: (key below HKCU, value name or "" for the default, string data).
pub type PlanEntry = (String, String, String);

/// Everything `--register` writes, all below HKCU.
pub fn registry_plan(exe: &Path) -> Vec<PlanEntry> {
    let exe = exe.display();
    let prog = format!(r"Software\Classes\{PROG_ID}");
    let mut plan: Vec<PlanEntry> = vec![
        (prog.clone(), "".into(), "Text file (scrivi)".into()),
        (
            format!(r"{prog}\DefaultIcon"),
            "".into(),
            format!("\"{exe}\",0"),
        ),
        (
            format!(r"{prog}\shell\open\command"),
            "".into(),
            format!("\"{exe}\" \"%1\""),
        ),
        // Explorer's Edit verb comes from the default ProgID's shell\edit.
        (
            format!(r"{prog}\shell\edit\command"),
            "".into(),
            format!("\"{exe}\" \"%1\""),
        ),
        (
            CAPABILITIES.into(),
            "ApplicationName".into(),
            APP_NAME.into(),
        ),
        (
            CAPABILITIES.into(),
            "ApplicationDescription".into(),
            "A minimal text editor that replaces Notepad.".into(),
        ),
        (REGISTERED_APPS.into(), APP_NAME.into(), CAPABILITIES.into()),
    ];
    for ext in EXTENSIONS {
        plan.push((
            format!(r"{CAPABILITIES}\FileAssociations"),
            (*ext).into(),
            PROG_ID.into(),
        ));
        plan.push((
            format!(r"Software\Classes\{ext}\OpenWithProgids"),
            PROG_ID.into(),
            "".into(),
        ));
    }
    plan
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!(r"{prefix}\{key}")
    }
}

/// Writes the plan below `HKCU\<prefix>` (empty prefix = the real registry).
pub fn apply(prefix: &str, plan: &[PlanEntry]) -> Result<(), String> {
    for (key, name, data) in plan {
        let (k, n, d) = (wide(&join(prefix, key)), wide(name), wide(data));
        let name_ptr = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(n.as_ptr())
        };
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(k.as_ptr()),
                name_ptr,
                REG_SZ.0,
                Some(d.as_ptr().cast()),
                (d.len() * 2) as u32,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(format!("writing {key}: error {}", err.0));
        }
    }
    Ok(())
}

/// Number of subkeys and values of a key, or `None` if it doesn't exist.
fn counts(subkey: &str) -> Option<(u32, u32)> {
    let k = wide(subkey);
    unsafe {
        let mut h = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            None,
            KEY_READ,
            &mut h,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let (mut subs, mut vals) = (0u32, 0u32);
        let err = RegQueryInfoKeyW(
            h,
            None,
            None,
            None,
            Some(&mut subs),
            None,
            None,
            Some(&mut vals),
            None,
            None,
            None,
            None,
        );
        let _ = RegCloseKey(h);
        (err == ERROR_SUCCESS).then_some((subs, vals))
    }
}

/// Keys shared with Windows or other apps, never deleted even when empty (the
/// real registry only; under a test prefix everything is ours).
fn shared(key: &str) -> bool {
    matches!(key, "Software" | r"Software\Classes" | REGISTERED_APPS)
}

/// Removes exactly the values the plan writes, then every key it created that
/// is now empty (deepest first). Other apps' values and keys stay.
pub fn remove(prefix: &str, plan: &[PlanEntry]) -> Result<(), String> {
    let mut keys: Vec<&str> = Vec::new();
    for (key, name, _) in plan {
        let (k, n) = (wide(&join(prefix, key)), wide(name));
        let name_ptr = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(n.as_ptr())
        };
        let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), name_ptr) };
        // A missing key reports PATH_NOT_FOUND, a missing value FILE_NOT_FOUND.
        if err != ERROR_SUCCESS && err != ERROR_FILE_NOT_FOUND && err != ERROR_PATH_NOT_FOUND {
            return Err(format!("removing {key}: error {}", err.0));
        }
        // The key and each ancestor are candidates for pruning.
        let mut cur = key.as_str();
        loop {
            if !keys.contains(&cur) {
                keys.push(cur);
            }
            match cur.rfind('\\') {
                Some(i) => cur = &cur[..i],
                None => break,
            }
        }
    }
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for key in keys {
        if prefix.is_empty() && shared(key) {
            continue;
        }
        let full = join(prefix, key);
        if counts(&full) == Some((0, 0)) {
            let k = wide(&full);
            let err = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr())) };
            if err != ERROR_SUCCESS {
                return Err(format!("deleting {key}: error {}", err.0));
            }
        }
    }
    Ok(())
}

/// Debug builds only: `SCRIVI_TEST_REGISTRY_PREFIX` redirects --register and
/// --unregister to a throwaway HKCU subtree and skips Explorer and Settings.
/// Release builds ignore the variable.
#[cfg(debug_assertions)]
fn test_prefix() -> Option<String> {
    std::env::var("SCRIVI_TEST_REGISTRY_PREFIX")
        .ok()
        .filter(|p| !p.is_empty())
}

#[cfg(not(debug_assertions))]
fn test_prefix() -> Option<String> {
    None
}

/// `--register`: write the plan for the running exe, tell Explorer, then open
/// the Default apps page where the user picks scrivi once.
pub fn register() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return fail(&format!("cannot find scrivi.exe: {e}")),
    };
    if let Some(prefix) = test_prefix() {
        return match apply(&prefix, &registry_plan(&exe)) {
            Ok(()) => 0,
            Err(e) => fail(&e),
        };
    }
    if let Err(e) = apply("", &registry_plan(&exe)) {
        return fail(&e);
    }
    notify_assoc_changed();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:defaultapps?registeredAppUser=Scrivi"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
    0
}

/// `--unregister`: remove exactly what `--register` wrote.
pub fn unregister() -> i32 {
    let exe = std::env::current_exe().unwrap_or_default();
    if let Some(prefix) = test_prefix() {
        return match remove(&prefix, &registry_plan(&exe)) {
            Ok(()) => 0,
            Err(e) => fail(&e),
        };
    }
    if let Err(e) = remove("", &registry_plan(&exe)) {
        return fail(&e);
    }
    notify_assoc_changed();
    0
}

fn notify_assoc_changed() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

fn fail(msg: &str) -> i32 {
    eprintln!("scrivi: {msg}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use windows::Win32::System::Registry::{RegDeleteTreeW, RegGetValueW, RRF_RT_REG_SZ};

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Apps\scrivi\scrivi.exe")
    }

    fn find<'a>(plan: &'a [PlanEntry], key: &str, name: &str) -> Option<&'a str> {
        plan.iter()
            .find(|(k, n, _)| k == key && n == name)
            .map(|(_, _, d)| d.as_str())
    }

    #[test]
    fn open_and_edit_commands_quote_exe_and_argument() {
        let plan = registry_plan(&exe());
        let want = r#""C:\Apps\scrivi\scrivi.exe" "%1""#;
        assert_eq!(
            find(
                &plan,
                r"Software\Classes\Scrivi.TextFile\shell\open\command",
                ""
            ),
            Some(want)
        );
        assert_eq!(
            find(
                &plan,
                r"Software\Classes\Scrivi.TextFile\shell\edit\command",
                ""
            ),
            Some(want)
        );
        assert_eq!(
            find(&plan, r"Software\Classes\Scrivi.TextFile\DefaultIcon", ""),
            Some(r#""C:\Apps\scrivi\scrivi.exe",0"#)
        );
    }

    #[test]
    fn every_extension_is_a_capability_and_an_open_with_candidate() {
        let plan = registry_plan(&exe());
        for ext in EXTENSIONS {
            assert_eq!(
                find(&plan, r"Software\Scrivi\Capabilities\FileAssociations", ext),
                Some("Scrivi.TextFile")
            );
            assert_eq!(
                find(
                    &plan,
                    &format!(r"Software\Classes\{ext}\OpenWithProgids"),
                    "Scrivi.TextFile"
                ),
                Some("")
            );
        }
        for ext in [
            ".txt",
            ".md",
            ".markdown",
            ".log",
            ".ini",
            ".json",
            ".csv",
            ".env",
            ".cfg",
            ".conf",
            ".yaml",
            ".yml",
            ".toml",
            ".xml",
        ] {
            assert!(EXTENSIONS.contains(&ext), "{ext}");
        }
    }

    #[test]
    fn registered_application_points_at_capabilities() {
        let plan = registry_plan(&exe());
        assert_eq!(
            find(&plan, r"Software\RegisteredApplications", "Scrivi"),
            Some(r"Software\Scrivi\Capabilities")
        );
        assert_eq!(
            find(&plan, r"Software\Scrivi\Capabilities", "ApplicationName"),
            Some("Scrivi")
        );
        assert!(find(
            &plan,
            r"Software\Scrivi\Capabilities",
            "ApplicationDescription"
        )
        .is_some());
    }

    #[test]
    fn plan_never_touches_user_choice_hashes_or_extension_defaults() {
        let plan = registry_plan(&exe());
        for (key, name, _) in &plan {
            assert!(key.starts_with(r"Software\"), "{key}");
            assert!(
                !key.contains("UserChoice") && !key.contains("FileExts"),
                "{key}"
            );
            assert!(!name.to_lowercase().contains("hash"), "{name}");
            // `.ext` itself keeps its default; only OpenWithProgids is written.
            if key.starts_with(r"Software\Classes\.") {
                assert!(key.ends_with(r"\OpenWithProgids"), "{key}");
            }
        }
        let mut seen = std::collections::HashSet::new();
        assert!(plan
            .iter()
            .all(|(k, n, _)| seen.insert((k.clone(), n.clone()))));
    }

    // --- registry round trips, in a throwaway HKCU subtree ---

    fn scratch() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!(r"Software\ScriviTest-{}-{nanos}", std::process::id())
    }

    fn delete_tree(root: &str) {
        let k = wide(root);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
        }
    }

    fn read(full_key: &str, name: &str) -> Option<String> {
        let (k, n) = (wide(full_key), wide(name));
        let name_ptr = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(n.as_ptr())
        };
        let mut buf = [0u16; 512];
        let mut len = std::mem::size_of_val(&buf) as u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(k.as_ptr()),
                name_ptr,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        (err == ERROR_SUCCESS).then(|| {
            let chars = (len as usize / 2).saturating_sub(1);
            String::from_utf16_lossy(&buf[..chars])
        })
    }

    #[test]
    fn apply_writes_the_plan_and_remove_leaves_nothing() {
        let root = scratch();
        let plan = registry_plan(&exe());
        let applied = apply(&root, &plan);
        let written: Vec<_> = plan
            .iter()
            .map(|(k, n, d)| read(&join(&root, k), n).as_deref() == Some(d.as_str()))
            .collect();
        let removed = remove(&root, &plan);
        // Nothing may remain below the scratch root, not even empty keys.
        let left = counts(&root);
        delete_tree(&root);
        applied.unwrap();
        assert!(written.iter().all(|&ok| ok), "not every value read back");
        removed.unwrap();
        assert!(
            left.is_none() || left == Some((0, 0)),
            "left behind: {left:?}"
        );
    }

    #[test]
    fn remove_keeps_other_apps_values_and_keys() {
        let root = scratch();
        let plan = registry_plan(&exe());
        let key = r"Software\Classes\.txt\OpenWithProgids";
        let other = vec![(key.to_string(), "Other.App".to_string(), String::new())];
        apply(&root, &plan).unwrap();
        apply(&root, &other).unwrap();
        remove(&root, &plan).unwrap();
        let kept = read(&join(&root, key), "Other.App");
        let gone = read(&join(&root, key), "Scrivi.TextFile");
        delete_tree(&root);
        assert_eq!(kept.as_deref(), Some(""));
        assert_eq!(gone, None);
    }
}
