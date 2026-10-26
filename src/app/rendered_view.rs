//! The Rendered view's glue (SCRIVI-21): WebView2 lives only while some Tab
//! is in the Rendered view (spec §6). `rendered::webview` owns WebView2; this
//! shows, hides or closes it after each `command`, keeps the editor read-only
//! and hidden behind the page, and handles the messages its handlers post.

use std::cell::{Cell, OnceCell};
use std::path::Path;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CopyAcceleratorTableW, DestroyAcceleratorTable, GetForegroundWindow, IsWindowVisible, ShowWindow, ACCEL,
    SW_HIDE, SW_SHOW, SW_SHOWNORMAL,
};

use super::{accelerators, apply_settings, cmd, command, message, perform, wide, App};
use crate::chrome::Rect;
use crate::editor::{sci, Editor};
use crate::proto::{WM_RENDERED_LINK, WM_RENDERED_SHOWN};
use crate::rendered::{self, webview};
use crate::tabs::{Action, StatusNote, Tab, TabId, View};
thread_local! {
    /// The editor is read-only behind the Rendered view (and hidden once the page is up).
    static BEHIND_PAGE: Cell<bool> = const { Cell::new(false) };
    /// The Window's accelerators, for keys pressed while the page has the focus.
    static KEYS: OnceCell<Vec<ACCEL>> = const { OnceCell::new() };
}

/// The active Tab is in the Rendered view: the editor behind it is read-only,
/// and hidden once the page is up.
pub(super) fn behind_page() -> bool {
    BEHIND_PAGE.get()
}

pub(super) fn rect_of(r: Rect) -> RECT {
    RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/// Show the active Tab's page if it is in the Rendered view; otherwise hide
/// WebView2 while another Tab still is, or close it when none is.
pub(super) fn sync_rendered(a: &App) {
    let (rendered, any, path) = {
        let tabs = a.tabs.borrow();
        let tab = tabs.active().and_then(|id| tabs.get(id));
        let rendered = tab.filter(|t| t.view() == View::Rendered).map(Tab::id);
        (rendered, tabs.any_rendered(), tab.and_then(|t| t.path().map(Path::to_path_buf)))
    };
    if rendered.is_some() {
        // No text to search in the page: the find bar goes.
        super::find::close(a);
    }
    let Some(id) = rendered.filter(|id| a.shown.get() == Some(*id)) else {
        if BEHIND_PAGE.replace(false) {
            let page_was_up = webview::painted();
            unsafe {
                let _ = ShowWindow(a.editor.hwnd(), SW_SHOW);
            }
            if let Some(id) = a.shown.get() {
                // The read-only state goes back to the Tab's own.
                a.applied.set(None);
                apply_settings(a, id);
            }
            if page_was_up || unsafe { GetFocus() }.is_invalid() {
                unsafe {
                    let _ = SetFocus(Some(a.editor.hwnd()));
                }
            }
        }
        if any {
            webview::hide();
        } else {
            webview::close();
        }
        return;
    };
    let folder = webview::folder_for(path.as_deref());
    let text = editor_text(&a.editor);
    match rendered::to_html(&text, &folder) {
        Ok(page) => {
            // Until the page is up the source stays in sight, but can't change.
            // Every time: showing another rendered Tab reapplied its own settings.
            BEHIND_PAGE.set(true);
            a.editor.set_read_only(true);
            let keys = KEYS.with(|k| k.get_or_init(accelerator_entries).clone());
            let bounds = rect_of(a.ui.borrow().layout.editor);
            webview::show(a.hwnd, &keys, cmd::SHOW_SOURCE, page, bounds);
            if webview::painted() {
                // A page already loaded (back to a Tab shown before) is up at once.
                cover_editor(a);
            } else {
                // A new page is loading: the read-only source shows until it is up.
                uncover_editor(a);
            }
        }
        Err(rendered::TooLarge) => {
            command(a, |t| t.show_source(id));
            perform(a, Action::Flash(StatusNote::TooLargeToRender));
        }
    }
}

/// The shown document's text. Scintilla holds UTF-8 (`SC_CP_UTF8`).
fn editor_text(e: &Editor) -> String {
    let len = e.text_len();
    let ptr = e.call(sci::SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
    if len == 0 || ptr.is_null() {
        return String::new();
    }
    // The pointer stays valid until the document next changes, after this copy.
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

/// The entries of `accelerators()`, the same table the message loop uses.
fn accelerator_entries() -> Vec<ACCEL> {
    unsafe {
        let table = accelerators();
        let n = CopyAcceleratorTableW(table, None);
        let mut entries = vec![ACCEL::default(); n.max(0) as usize];
        CopyAcceleratorTableW(table, Some(&mut entries));
        let _ = DestroyAcceleratorTable(table);
        entries
    }
}

/// Once the page is on screen it covers the editor band: hide the editor
/// behind it and give the page the focus.
fn cover_editor(a: &App) {
    let editor = a.editor.hwnd();
    unsafe {
        if !webview::painted() || !BEHIND_PAGE.get() || !IsWindowVisible(editor).as_bool() {
            return;
        }
        let had_focus = GetFocus() == editor;
        let _ = ShowWindow(editor, SW_HIDE);
        if had_focus || GetForegroundWindow() == a.hwnd {
            webview::focus();
        }
    }
}

/// The page is out of sight while its next page loads: the editor shows again.
fn uncover_editor(a: &App) {
    let editor = a.editor.hwnd();
    unsafe {
        if IsWindowVisible(editor).as_bool() {
            return;
        }
        let _ = ShowWindow(editor, SW_SHOW);
        if GetForegroundWindow() == a.hwnd {
            let _ = SetFocus(Some(editor));
        }
    }
}

/// Restored from minimised: the page is visible again and takes the focus
/// back, which `WM_SETFOCUS` may have given to the hidden editor.
pub(super) fn refocus(a: &App) {
    let focus = unsafe { GetFocus() };
    if webview::painted() && (focus == a.editor.hwnd() || focus == a.hwnd) {
        webview::focus();
    }
}

/// The messages `rendered::webview` posts from WebView2's handlers.
pub(super) unsafe fn on_rendered(a: &App, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_RENDERED_SHOWN => {
                webview::reveal();
                cover_editor(a);
            }
            WM_RENDERED_LINK => {
                let uri = Box::from_raw(lp.0 as *mut String);
                // Checked again here: only web and mail links leave scrivi.
                if rendered::opens_outside(&uri) {
                    let uri = wide(&uri);
                    ShellExecuteW(Some(a.hwnd), w!("open"), PCWSTR(uri.as_ptr()), None, None, SW_SHOWNORMAL);
                }
            }
            _ => {
                // Back to the Source view: every rendered Tab if WebView2
                // couldn't start, the active one if its page was refused.
                let failed_start = wp.0 == webview::FAILED_TO_START;
                command(a, |t| {
                    let ids: Vec<TabId> = if failed_start {
                        t.iter().filter(|x| x.view() == View::Rendered).map(Tab::id).collect()
                    } else {
                        t.active().into_iter().collect()
                    };
                    ids.into_iter().flat_map(|id| t.show_source(id)).collect()
                });
                if failed_start {
                    message(a, "The Rendered view needs the Microsoft Edge WebView2 Runtime, and it didn't start.");
                } else {
                    perform(a, Action::Flash(StatusNote::TooLargeToRender));
                }
            }
        }
        LRESULT(0)
    }
}
