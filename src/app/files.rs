//! Opening and saving (SCRIVI-16): the Open and Save As dialogs, writing a
//! Tab through `textfile::encode` and [`atomic::write`], Reopen with
//! encoding, files dropped on the Window, and Scintilla's save point.
//!
//! Every dialog here pumps messages: nothing is borrowed across one.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8, WINDOWS_1252};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::GetACP;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TDCBF_CANCEL_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_POSITION_RELATIVE_TO_WINDOW,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    DragFinish, DragQueryFileW, FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
    IShellItem, SHCreateItemFromParsingName, SHGetKnownFolderItem, FOLDERID_Documents, FOS_ALLOWMULTISELECT,
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, HDROP, KF_FLAG_DEFAULT,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

use super::{atomic, command, headless, in_document, message, wide, App};
use crate::editor::{sci, Editor};
use crate::tabs::{Action, FileId, FileStat, SaveAsDefault, SaveAsFolder, StatusNote, Tab, TabId};
use crate::textfile::{self, Encoding, Eol};

/// A Scintilla message with no arguments.
pub(super) fn send(e: &Editor, msg: u32) -> isize {
    unsafe { SendMessageW(e.hwnd(), msg, Some(WPARAM(0)), Some(LPARAM(0))).0 }
}

/// COM for the file dialogs, once per thread.
fn com() {
    thread_local!(static DONE: Cell<bool> = const { Cell::new(false) });
    if !DONE.replace(true) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        }
    }
}

/// The folder of the active Tab's file, if it has one.
fn active_folder(a: &App) -> Option<PathBuf> {
    let tabs = a.tabs.borrow();
    let tab = tabs.get(tabs.active()?)?;
    tab.path()?.parent().map(Path::to_path_buf)
}

fn shell_item(folder: &Path) -> Option<IShellItem> {
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(folder), None).ok() }
}

/// A shell item's file-system path.
fn path_of(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// The identity of the file at `path`, for `tabs` to tell two paths to one
/// file apart from two files. `None` if it can't be read (a new file).
pub fn file_id(path: &Path) -> Option<FileId> {
    atomic::file_info(path).and_then(|i| i.id)
}

/// Open each path in a Tab, or switch to the Tab already on that file.
pub fn open_paths(a: &App, paths: &[PathBuf]) {
    let with_ids: Vec<_> = paths.iter().map(|p| (p.clone(), file_id(p))).collect();
    command(a, |t| t.open_with_ids(&with_ids));
}

/// File › Open… (Ctrl+O): pick one or more files, each opens in a Tab.
pub fn open(a: &App) {
    if headless() {
        return;
    }
    let Some(paths) = pick_files(a) else { return };
    if !paths.is_empty() {
        open_paths(a, &paths);
    }
}

fn pick_files(a: &App) -> Option<Vec<PathBuf>> {
    com();
    let folder = active_folder(a);
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        let types = [
            COMDLG_FILTERSPEC { pszName: w!("All files (*.*)"), pszSpec: w!("*.*") },
            COMDLG_FILTERSPEC { pszName: w!("Markdown (*.md)"), pszSpec: w!("*.md;*.markdown") },
            COMDLG_FILTERSPEC { pszName: w!("Text (*.txt)"), pszSpec: w!("*.txt") },
        ];
        dialog.SetFileTypes(&types).ok()?;
        if let Some(item) = folder.as_deref().and_then(shell_item) {
            let _ = dialog.SetFolder(&item);
        }
        // Cancel is an error too.
        dialog.Show(Some(a.hwnd)).ok()?;
        let results = dialog.GetResults().ok()?;
        let count = results.GetCount().ok()?;
        Some((0..count).filter_map(|i| results.GetItemAt(i).ok()).filter_map(|item| path_of(&item)).collect())
    }
}

/// `Action::AskSaveAs`: the Save As dialog, starting at `default`.
pub fn ask_save_as(a: &App, id: TabId, default: &SaveAsDefault) {
    match pick_save_path(a, default) {
        Some(path) => {
            let file_id = file_id(&path);
            command(a, |t| t.save_as_chosen_with_id(id, path, file_id));
        }
        None => command(a, |t| t.save_failed(id)),
    }
}

/// The filter list of the Save As dialog, and which one an extension picks.
const SAVE_TYPES: [(PCWSTR, PCWSTR); 3] = [
    (w!("Markdown (*.md)"), w!("*.md;*.markdown")),
    (w!("Text (*.txt)"), w!("*.txt")),
    (w!("All files (*.*)"), w!("*.*")),
];

/// The 1-based filter for a file name, and the extension a typed name
/// without one gets.
fn save_type(file_name: &str) -> (u32, String) {
    let ext = Path::new(file_name).extension().map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        None => (1, "md".into()),
        Some("md" | "markdown") => (1, ext.unwrap_or_default()),
        Some("txt") => (2, "txt".into()),
        Some(other) => (3, other.into()),
    }
}

fn pick_save_path(a: &App, default: &SaveAsDefault) -> Option<PathBuf> {
    if headless() {
        return None;
    }
    com();
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
        let types = SAVE_TYPES.map(|(name, spec)| COMDLG_FILTERSPEC { pszName: name, pszSpec: spec });
        dialog.SetFileTypes(&types).ok()?;
        let (index, ext) = save_type(&default.file_name);
        let _ = dialog.SetFileTypeIndex(index);
        let _ = dialog.SetDefaultExtension(&HSTRING::from(ext));
        let _ = dialog.SetFileName(&HSTRING::from(default.file_name.as_str()));
        let folder: Option<IShellItem> = match &default.folder {
            SaveAsFolder::Documents => SHGetKnownFolderItem(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok(),
            SaveAsFolder::Folder(folder) => shell_item(folder),
        };
        if let Some(folder) = folder {
            let _ = dialog.SetFolder(&folder);
        }
        let dialog: IFileDialog = windows::core::Interface::cast(&dialog).ok()?;
        dialog.Show(Some(a.hwnd)).ok()?;
        path_of(&dialog.GetResult().ok()?)
    }
}

/// The text of Tab `id`'s document, shown or not.
fn text_of(a: &App, id: TabId) -> Option<String> {
    let mut text = None;
    in_document(a, id, |e| {
        let len = send(e, sci::SCI_GETLENGTH).max(0) as usize;
        let ptr = send(e, sci::SCI_GETCHARACTERPOINTER) as *const u8;
        text = Some(if ptr.is_null() || len == 0 {
            String::new()
        } else {
            // Scintilla holds UTF-8 (SC_CP_UTF8), filled from Rust strings.
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
        });
    });
    text
}

/// `Action::Write`: encode Tab `id`'s text and write it to `path`. A text the
/// encoding can't hold asks to save as UTF-8 instead.
pub fn write(a: &App, id: TabId, path: &Path, encoding: Encoding, eol: Eol) {
    let Some(text) = text_of(a, id) else {
        command(a, |t| t.save_failed(id));
        return;
    };
    let bytes = match textfile::encode(&text, encoding, eol) {
        Ok(bytes) => bytes,
        Err(bad) => {
            drop(text);
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
            let chars: String = bad.chars.iter().take(12).flat_map(|c| [*c, ' ']).collect();
            let more = if bad.chars.len() > 12 { "\u{2026}" } else { "" };
            let utf8 = confirm(
                a,
                &format!("{name} can\u{2019}t be saved as {encoding}"),
                &format!(
                    "It has characters {encoding} can\u{2019}t hold: {}{more}\n\n\
                     Save it as UTF-8 instead? Nothing is lost.",
                    chars.trim_end()
                ),
                "Save as UTF-8",
            );
            if !utf8 {
                command(a, |t| t.save_failed(id));
                return;
            }
            command(a, |t| t.use_utf8_instead(id));
            return write(a, id, path, Encoding::Utf8, eol);
        }
    };
    drop(text);
    match atomic::write(path, &bytes) {
        Ok(written) => {
            let stat = super::stat_of(path)
                .unwrap_or(FileStat { size: bytes.len() as u64, modified: SystemTime::now() });
            // Before `saved`: a close waiting on this save releases the document.
            in_document(a, id, |e| {
                send(e, sci::SCI_SETSAVEPOINT);
            });
            let file_id = file_id(path);
            command(a, |t| t.saved_with_id(id, path.to_path_buf(), stat, file_id, Instant::now()));
            if written == atomic::Written::InPlace {
                command(a, |_| vec![Action::Flash(StatusNote::SavedInPlace)]);
            }
        }
        Err(e) => {
            // The outcome goes in before the message box pumps messages.
            command(a, |t| t.save_failed(id));
            message(a, &format!("Couldn\u{2019}t save {}.\n\n{e}", path.display()));
        }
    }
}

/// One custom button and Cancel. Pumps messages, so nothing may be borrowed
/// across it.
fn confirm(a: &App, instruction: &str, content: &str, yes: &str) -> bool {
    const YES: i32 = 100;
    if headless() {
        return false;
    }
    let (instruction, content, yes) = (wide(instruction), wide(content), wide(yes));
    let buttons = [TASKDIALOG_BUTTON { nButtonID: YES, pszButtonText: PCWSTR(yes.as_ptr()) }];
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: a.hwnd,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: w!("scrivi"),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: YES,
        ..Default::default()
    };
    let mut pressed = 0;
    unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) }.is_ok() && pressed == YES
}

/// The encodings Reopen with encoding… offers: the menu's five, the Tab's
/// own if the menu lacks it, and the system ANSI code page if it isn't 1252.
fn reopen_choices(current: Encoding) -> Vec<Encoding> {
    let mut choices = Encoding::MENU.to_vec();
    let ansi = codepage::to_encoding(unsafe { GetACP() } as u16)
        .filter(|e| ![WINDOWS_1252, UTF_8, UTF_16LE, UTF_16BE].contains(e))
        .map(Encoding::Other);
    for extra in [Some(current), ansi].into_iter().flatten() {
        if !choices.contains(&extra) {
            choices.push(extra);
        }
    }
    choices
}

/// Encoding › Reopen with encoding…: pick an encoding, then reread the
/// active Tab's file in it (asking first if that drops unsaved edits).
pub fn reopen(a: &App) {
    let Some((id, name, current)) = ({
        let tabs = a.tabs.borrow();
        tabs.active().and_then(|id| tabs.get(id)).filter(|t| !t.is_untitled()).map(|t| (t.id(), t.name(), t.encoding()))
    }) else {
        return;
    };
    if let Some(encoding) = pick_encoding(a, &name, current) {
        command(a, |t| t.reopen_with_encoding(id, encoding));
    }
}

/// A radio list of encodings with Reopen and Cancel.
fn pick_encoding(a: &App, name: &str, current: Encoding) -> Option<Encoding> {
    const REOPEN: i32 = 100;
    const FIRST: i32 = 200;
    if headless() {
        return None;
    }
    let choices = reopen_choices(current);
    let labels: Vec<Vec<u16>> = choices.iter().map(|e| wide(e.name())).collect();
    let radios: Vec<TASKDIALOG_BUTTON> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| TASKDIALOG_BUTTON { nButtonID: FIRST + i as i32, pszButtonText: PCWSTR(l.as_ptr()) })
        .collect();
    let instruction = wide(&format!("Reopen {name} with encoding"));
    let content = wide("Reads the file again from disk in the encoding you pick.");
    let reopen = wide("Reopen");
    let buttons = [TASKDIALOG_BUTTON { nButtonID: REOPEN, pszButtonText: PCWSTR(reopen.as_ptr()) }];
    let at = choices.iter().position(|e| *e == current).unwrap_or(0);
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: a.hwnd,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: w!("scrivi"),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: REOPEN,
        cRadioButtons: radios.len() as u32,
        pRadioButtons: radios.as_ptr(),
        nDefaultRadioButton: FIRST + at as i32,
        ..Default::default()
    };
    let (mut pressed, mut radio) = (0, 0);
    unsafe { TaskDialogIndirect(&config, Some(&mut pressed), Some(&mut radio), None) }.ok()?;
    (pressed == REOPEN).then(|| choices.get((radio - FIRST) as usize).copied()).flatten()
}

/// `Action::AskDiscardForReopen`.
pub fn ask_discard_for_reopen(a: &App, id: TabId, encoding: Encoding) {
    let Some(name) = a.tabs.borrow().get(id).map(Tab::name) else { return };
    let yes = confirm(
        a,
        &format!("Discard your unsaved edits to {name}?"),
        &format!("Reopening reads the file again from disk as {encoding}."),
        "Discard and reopen",
    );
    if yes {
        command(a, |t| t.reopen_confirmed(id, encoding));
    }
}

/// `WM_DROPFILES`: each dropped file opens in a Tab, as with Open….
pub fn dropped(a: &App, drop: HDROP) {
    let paths: Vec<PathBuf> = unsafe {
        let count = DragQueryFileW(drop, u32::MAX, None);
        let paths = (0..count)
            .map(|i| {
                let mut buf = vec![0u16; DragQueryFileW(drop, i, None) as usize + 1];
                let len = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                PathBuf::from(String::from_utf16_lossy(&buf[..len]))
            })
            .collect();
        DragFinish(drop);
        paths
    };
    // Folders have no text.
    let files: Vec<PathBuf> = paths.into_iter().filter(|p| !p.is_dir()).collect();
    if !files.is_empty() {
        open_paths(a, &files);
    }
}

/// `SCN_SAVEPOINTLEFT` / `SCN_SAVEPOINTREACHED` from the shown document.
pub fn save_point(a: &App, left: bool) {
    if let Some(id) = a.shown.get() {
        command(a, |t| if left { t.save_point_left(id) } else { t.save_point_reached(id) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_as_picks_the_filter_from_the_extension() {
        assert_eq!(save_type("Untitled 1.md"), (1, "md".to_string()));
        assert_eq!(save_type("notes.MARKDOWN"), (1, "markdown".to_string()));
        assert_eq!(save_type("todo.txt"), (2, "txt".to_string()));
        assert_eq!(save_type("app.log"), (3, "log".to_string()));
        assert_eq!(save_type("README"), (1, "md".to_string()));
    }

    #[test]
    fn reopen_offers_the_menus_encodings_and_the_tabs_own() {
        let choices = reopen_choices(Encoding::Other(encoding_rs::SHIFT_JIS));
        assert_eq!(choices[..5], Encoding::MENU);
        assert!(choices.contains(&Encoding::Other(encoding_rs::SHIFT_JIS)));
        let plain = reopen_choices(Encoding::Utf8);
        assert!(plain.iter().filter(|e| **e == Encoding::Utf8).count() == 1);
    }

    /// Open a 200 MB log the way `Action::Load` does (read, decode, create the
    /// document, fill it) in a real, never shown Scintilla control. Run:
    /// `SCRIVI_TIMING_FILE=<path> cargo test --release load_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn load_timing() {
        use std::time::Instant;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::HWND_MESSAGE;
        let path = PathBuf::from(std::env::var("SCRIVI_TIMING_FILE").expect("SCRIVI_TIMING_FILE"));
        let hinstance = unsafe { GetModuleHandleW(None).unwrap() }.into();
        assert!(Editor::register_class(hinstance));
        let parent: HWND = HWND_MESSAGE;
        let e = Editor::create(parent, hinstance, 96).expect("editor");
        let t = Instant::now();
        let (bytes, stat) = super::super::read_file(&path).unwrap();
        let read = t.elapsed();
        let decoded = textfile::decode_owned(bytes, 1252);
        let decode = t.elapsed();
        let large = textfile::is_large(stat.size);
        let doc = e.create_document(large);
        e.show_document(doc);
        e.set_eol_mode(decoded.eol);
        e.fill(&decoded.text, large);
        let total = t.elapsed();
        eprintln!(
            "{} MB, large={large}: read {read:?}, + decode {decode:?}, total {total:?}; {} bytes in the document",
            stat.size / (1024 * 1024),
            e.text_len()
        );
    }
}
