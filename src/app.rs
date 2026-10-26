//! The Window, its wndproc, the message loop and the glue between modules.
//!
//! The Window draws its own one-row title bar (ADR 0002): `WM_NCCALCSIZE`
//! drops the native caption but keeps the left, right and bottom resize
//! borders (and so the shadow); `WM_NCHITTEST` answers from the pure
//! `chrome::Layout::hit_test`. One Scintilla control fills the editor band.
//!
//! Every Tab rule lives in `tabs::Tabs`: `command` runs one of its commands,
//! then `perform`s the actions it returns. Each Tab owns a Scintilla
//! document; showing a Tab swaps its document into the one control and puts
//! back its selection and scroll.
//!
//! Debug builds append each request to the file named by `SCRIVI_TEST_LOG`,
//! if set, and keep the Window hidden then, so the launch tests stay headless.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::io::Read;
#[cfg(debug_assertions)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Globalization::GetACP;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, ScreenToClient, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, TaskDialogIndirect, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, TASKDIALOGCONFIG,
    TASKDIALOG_BUTTON, TDCBF_CANCEL_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_POSITION_RELATIVE_TO_WINDOW,
    TOOLTIPS_CLASSW, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_NEWTOOLRECTW, TTM_POP, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP,
    TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
    VK_ADD, VK_NUMPAD0, VK_OEM_MINUS, VK_OEM_PLUS, VK_SUBTRACT, VK_TAB,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::chrome::{Frame, Hit, Layout, Painter, Rect, Status, TabView, View};
use crate::editor::{sci, Caret, Editor, NotifyHeader, ViewState};
use crate::launch::{self, Request};
use crate::mdview::{self, Applied};
use crate::proto::{self, COPYDATA_REQUEST, ICON, RESTYLE_MS, SPELL_MS, TIMER_PREFS, TIMER_RESTYLE, TIMER_SPELL, WM_EDITOR_DPI, WM_HANDOFF};
use crate::proto::{WM_RENDERED_FAILED, WM_RENDERED_LINK, WM_RENDERED_SHOWN};
use crate::rendered::webview;
use crate::spellview::SpellView;
use crate::tabs::{Action, CloseReply, EditorDoc, FileStat, LoadedFile, StatusNote, Tab, TabId, Tabs};
use crate::textfile::{self, Encoding, Eol};

// SCRIVI-16: opening and saving, the menus. New code lives in these files.
mod atomic;
mod files;
mod find;
mod findview;
mod menu;
mod notice;
mod recent;
mod rendered_view;

use rendered_view::{on_rendered, rect_of, sync_rendered};

/// First size of the Window, in DIPs, before prefs (09) remembers one.
const START_W: f32 = 1100.0;
const START_H: f32 = 760.0;
const MIN_W: f32 = 480.0;
const MIN_H: f32 = 240.0;
/// Repaints asked for after failed paints, before giving up until the next
/// ordinary one.
const PAINT_RETRIES: u32 = 3;
/// How long an `Action::Flash` note stays in the status bar.
const FLASH: Duration = Duration::from_millis(2500);
const FLASH_TIMER: usize = 1;
/// `Action::Recheck` timers count up from here.
const FIRST_RECHECK_TIMER: usize = 0x100;
/// The tooltip's one tool: whichever Tab the mouse is over.
const TIP_TOOL: usize = 1;

/// `WM_COMMAND` ids, from the accelerators (and from 08's menus).
pub mod cmd {
    pub const NEW_TAB: u16 = 100;
    pub const CLOSE_TAB: u16 = 101;
    pub const NEXT_TAB: u16 = 102;
    pub const PREV_TAB: u16 = 103;
    // The menus (SCRIVI-16).
    pub const OPEN: u16 = 104;
    pub const SAVE: u16 = 105;
    pub const SAVE_AS: u16 = 106;
    pub const EXIT: u16 = 107;
    pub const UNDO: u16 = 110;
    pub const REDO: u16 = 111;
    pub const CUT: u16 = 112;
    pub const COPY: u16 = 113;
    pub const PASTE: u16 = 114;
    pub const SELECT_ALL: u16 = 115;
    pub const FIND: u16 = 116;
    pub const REPLACE: u16 = 117;
    pub const GO_TO_LINE: u16 = 118;
    /// Encoding ›: `ENCODING + i` picks `Encoding::MENU[i]`.
    pub const ENCODING: u16 = 120;
    /// The Tab's own encoding, when the menu does not list it: keeps it.
    pub const ENCODING_KEEP: u16 = 126;
    pub const REOPEN: u16 = 127;
    /// Line endings ›: `EOL + i` picks `Eol::MENU[i]`.
    pub const EOL: u16 = 130;
    pub const RENDERED_VIEW: u16 = 140;
    pub const WORD_WRAP: u16 = 141;
    pub const ZOOM_IN: u16 = 142;
    pub const ZOOM_OUT: u16 = 143;
    pub const ZOOM_RESET: u16 = 144;
    /// Esc in the Rendered view: back to the Source view (13).
    pub const SHOW_SOURCE: u16 = 145;
    /// File › Recent files: `RECENT + i` is the i-th (SCRIVI-23).
    pub const RECENT: u16 = 200;
}

/// What the chrome shows; borrowed only for short stretches that make no
/// Win32 call which could send a message back (spec §13).
struct Ui {
    layout: Layout,
    menu_text: [f32; 3],
    caret: Caret,
    zoom: i32,
    digits: u32,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    /// The Tab the middle button went down on: it closes if it comes up there.
    middle: Option<TabId>,
    /// How far the Tabs are scrolled, in pixels; the layout clamps it.
    tab_scroll: i32,
    /// The active Tab the strip last scrolled into view.
    revealed: Option<TabId>,
    /// A note shown for a moment (`Action::Flash`).
    flash: Option<StatusNote>,
    /// The Tab whose full path the tooltip holds.
    tip: Option<TabId>,
    title: String,
    tracking: bool,
    tracking_nc: bool,
    active: bool,
    /// Paints failed in a row, for `PAINT_RETRIES`.
    paint_failures: u32,
}

struct App {
    hwnd: HWND,
    editor: Editor,
    tooltip: HWND,
    painter: RefCell<Painter>,
    ui: RefCell<Ui>,
    tabs: RefCell<Tabs>,
    /// Selection and scroll of each Tab, kept while another Tab is shown.
    views: RefCell<HashMap<TabId, ViewState>>,
    /// The Tab whose document the editor shows.
    shown: Cell<Option<TabId>>,
    /// Set while scrivi itself changes a document's text: that is no edit.
    quiet: Cell<bool>,
    /// The line ending, read-only state and wrap last sent for the shown Tab.
    applied: Cell<Option<(Eol, bool, bool)>>,
    /// The Tabs whose documents hold markdown styles.
    styled: RefCell<Applied<TabId>>,
    /// Spell check: the checkers and the squiggles of the shown document.
    spell: SpellView,
    /// Pending `Action::Recheck`s: timer id and path.
    rechecks: RefCell<Vec<(usize, PathBuf)>>,
    next_timer: Cell<usize>,
    /// `Action::CloseWindow` destroyed the Window: perform nothing more.
    closing: Cell<bool>,
}

thread_local! {
    static APP: OnceCell<&'static App> = const { OnceCell::new() };
}

fn app() -> Option<&'static App> {
    APP.with(|a| a.get().copied())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32)
}

fn scale_of(hwnd: HWND) -> f32 {
    unsafe { GetDpiForWindow(hwnd) as f32 / 96.0 }
}

/// The native resize border (frame plus padding) at `dpi`, in pixels.
fn resize_border(dpi: u32) -> (i32, i32) {
    unsafe {
        let pad = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        (GetSystemMetricsForDpi(SM_CXFRAME, dpi) + pad, GetSystemMetricsForDpi(SM_CYFRAME, dpi) + pad)
    }
}

/// Debug builds started by the launch tests stay hidden, and ask nothing.
fn headless() -> bool {
    cfg!(debug_assertions) && std::env::var_os("SCRIVI_TEST_LOG").is_some()
}

/// Create the Window, apply `initial`, and pump messages until it closes.
/// Returns the process exit code.
pub fn run(initial: Request) -> i32 {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return 1 };
        let hinstance: HINSTANCE = module.into();
        if !Editor::register_class(hinstance) {
            return 1;
        }
        // The tooltip class.
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        let icon = |size: i32| {
            LoadImageW(Some(hinstance), ICON, IMAGE_ICON, size, size, LR_SHARED).map(|h| HICON(h.0)).unwrap_or_default()
        };
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hIcon: icon(GetSystemMetrics(SM_CXICON)),
            hIconSm: icon(GetSystemMetrics(SM_CXSMICON)),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: proto::class(),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return 1;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            proto::class(),
            PCWSTR::null(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinstance),
            None,
        ) else {
            return 1;
        };
        // An elevated launch hands over from a higher integrity level.
        let _ = ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None);
        // Files dropped from Explorer open in Tabs, elevated or not.
        const WM_COPYGLOBALDATA: u32 = 0x49;
        windows::Win32::UI::Shell::DragAcceptFiles(hwnd, true);
        for msg in [WM_DROPFILES, WM_COPYGLOBALDATA] {
            let _ = ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, None);
        }
        place(hwnd);
        let maximized = recent::restore_window(hwnd);

        let dpi = GetDpiForWindow(hwnd);
        let Ok(painter) = Painter::new(hinstance, dpi as f32 / 96.0) else { return 1 };
        let Some(editor) = Editor::create(hwnd, hinstance, dpi) else { return 1 };
        let tooltip = create_tooltip(hwnd, hinstance);
        let menu_text = painter.menu_text();
        let ui = Ui {
            layout: Layout::new(&Frame::default()),
            menu_text,
            caret: Caret::default(),
            zoom: 100,
            digits: 0,
            hover: None,
            pressed: None,
            middle: None,
            tab_scroll: 0,
            revealed: None,
            flash: None,
            tip: None,
            title: String::new(),
            tracking: false,
            tracking_nc: false,
            active: true,
            paint_failures: 0,
        };
        let a: &'static App = Box::leak(Box::new(App {
            hwnd,
            editor,
            tooltip,
            painter: RefCell::new(painter),
            ui: RefCell::new(ui),
            tabs: RefCell::new(Tabs::new()),
            views: RefCell::new(HashMap::new()),
            shown: Cell::new(None),
            quiet: Cell::new(false),
            applied: Cell::new(None),
            rechecks: RefCell::new(Vec::new()),
            next_timer: Cell::new(FIRST_RECHECK_TIMER),
            styled: RefCell::default(),
            spell: SpellView::new(),
            closing: Cell::new(false),
        }));
        APP.with(|c| {
            let _ = c.set(a);
        });
        recent::apply_view(a);
        relayout(a);
        // The start state: one empty Untitled tab, or the files the launch names.
        on_handoff(initial);
        if !headless() {
            let _ = ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW });
            let _ = SetFocus(Some(a.editor.hwnd()));
        }

        let accelerators = accelerators();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if menu::pre_translate(&msg) {
                continue;
            }
            if TranslateAcceleratorW(hwnd, accelerators, &msg) != 0 {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}

/// The Tab shortcuts (spec §4). They reach the Window as `WM_COMMAND`
/// whichever child has the focus, before Scintilla sees the keys.
fn accelerators() -> HACCEL {
    let ctrl = FCONTROL | FVIRTKEY;
    let key = |c: u8| c as u16;
    let table = [
        ACCEL { fVirt: ctrl, key: key(b'T'), cmd: cmd::NEW_TAB },
        // Ctrl+N: an alias of Ctrl+T, for Notepad hands (SCRIVI-16).
        ACCEL { fVirt: ctrl, key: key(b'N'), cmd: cmd::NEW_TAB },
        ACCEL { fVirt: ctrl, key: key(b'W'), cmd: cmd::CLOSE_TAB },
        ACCEL { fVirt: ctrl, key: VK_TAB.0, cmd: cmd::NEXT_TAB },
        ACCEL { fVirt: ctrl | FSHIFT, key: VK_TAB.0, cmd: cmd::PREV_TAB },
        ACCEL { fVirt: ctrl, key: key(b'O'), cmd: cmd::OPEN },
        ACCEL { fVirt: ctrl, key: key(b'S'), cmd: cmd::SAVE },
        ACCEL { fVirt: ctrl | FSHIFT, key: key(b'S'), cmd: cmd::SAVE_AS },
        ACCEL { fVirt: ctrl, key: key(b'E'), cmd: cmd::RENDERED_VIEW },
        // Find, wrap and zoom (SCRIVI-20). Ctrl++ is the plus key on layouts
        // where it is unshifted, and Shift and the = key on the others.
        ACCEL { fVirt: ctrl, key: key(b'F'), cmd: cmd::FIND },
        ACCEL { fVirt: ctrl, key: key(b'H'), cmd: cmd::REPLACE },
        ACCEL { fVirt: ctrl, key: key(b'G'), cmd: cmd::GO_TO_LINE },
        ACCEL { fVirt: FALT | FVIRTKEY, key: key(b'Z'), cmd: cmd::WORD_WRAP },
        ACCEL { fVirt: ctrl, key: VK_OEM_PLUS.0, cmd: cmd::ZOOM_IN },
        ACCEL { fVirt: ctrl | FSHIFT, key: VK_OEM_PLUS.0, cmd: cmd::ZOOM_IN },
        ACCEL { fVirt: ctrl, key: VK_ADD.0, cmd: cmd::ZOOM_IN },
        ACCEL { fVirt: ctrl, key: VK_OEM_MINUS.0, cmd: cmd::ZOOM_OUT },
        ACCEL { fVirt: ctrl, key: VK_SUBTRACT.0, cmd: cmd::ZOOM_OUT },
        ACCEL { fVirt: ctrl, key: key(b'0'), cmd: cmd::ZOOM_RESET },
        ACCEL { fVirt: ctrl, key: VK_NUMPAD0.0, cmd: cmd::ZOOM_RESET },
    ];
    unsafe { CreateAcceleratorTableW(&table) }.unwrap_or_default()
}

/// One tooltip for the strip; its tool follows the Tab under the mouse.
unsafe fn create_tooltip(owner: HWND, hinstance: HINSTANCE) -> HWND {
    unsafe {
        let Ok(tip) = CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            PCWSTR::null(),
            WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(owner),
            None,
            Some(hinstance),
            None,
        ) else {
            return HWND::default();
        };
        let mut text = wide("");
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            // The tooltip watches the Window's mouse messages itself.
            uFlags: TTF_SUBCLASS,
            hwnd: owner,
            uId: TIP_TOOL,
            lpszText: PWSTR(text.as_mut_ptr()),
            ..Default::default()
        };
        SendMessageW(tip, TTM_ADDTOOLW, None, Some(LPARAM(&info as *const _ as isize)));
        tip
    }
}

/// Size the new Window to `START_W` x `START_H` DIPs, within the work area,
/// and make Windows ask `WM_NCCALCSIZE` again.
unsafe fn place(hwnd: HWND) {
    unsafe {
        let s = scale_of(hwnd);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let work = if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1280, bottom: 800 }
        };
        let w = ((START_W * s) as i32).min(work.right - work.left);
        let h = ((START_H * s) as i32).min(work.bottom - work.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// A request from this launch (`run`'s `initial`) or a later one (`WM_HANDOFF`).
pub fn on_handoff(request: Request) {
    // Test hook, debug builds only.
    #[cfg(debug_assertions)]
    log_request(&request);
    let Some(a) = app() else { return };
    match request {
        Request::NewTab => command(a, Tabs::new_untitled),
        Request::Open(paths) => files::open_paths(a, &paths),
    }
    #[cfg(debug_assertions)]
    {
        log_tabs(a);
        log_editor(a);
    }
}

/// Test hook, debug builds only: rewrite the file named by `SCRIVI_TEST_EDITOR`
/// with how the shown document is set up, as `key=value` lines.
#[cfg(debug_assertions)]
fn log_editor(a: &App) {
    const SCI_GETREADONLY: u32 = 2140;
    const SCI_GETUNDOCOLLECTION: u32 = 2019;
    const SCI_GETWRAPMODE: u32 = 2269;
    let Some(path) = std::env::var_os("SCRIVI_TEST_EDITOR") else { return };
    let e = &a.editor;
    let text = format!(
        "name={}
read_only={}
undo={}
wrap={}
length={}
",
        a.shown.get().and_then(|id| a.tabs.borrow().get(id).map(Tab::name)).unwrap_or_default(),
        e.call(SCI_GETREADONLY, 0, 0),
        e.call(SCI_GETUNDOCOLLECTION, 0, 0),
        e.call(SCI_GETWRAPMODE, 0, 0),
        e.text_len(),
    );
    let tmp = PathBuf::from(&path).with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Test hook, debug builds only: rewrite the file named by `SCRIVI_TEST_TABS`
/// with the strip, one Tab name per line, the active one starred.
#[cfg(debug_assertions)]
fn log_tabs(a: &App) {
    let Some(path) = std::env::var_os("SCRIVI_TEST_TABS") else { return };
    let text: String = {
        let tabs = a.tabs.borrow();
        tabs.iter()
            .map(|t| format!("{}{}\n", if Some(t.id()) == tabs.active() { "*" } else { "" }, t.name()))
            .collect()
    };
    // Write then rename, so a reader never sees half a list.
    let tmp = PathBuf::from(&path).with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(debug_assertions)]
fn log_request(request: &Request) {
    let Some(path) = std::env::var_os("SCRIVI_TEST_LOG") else { return };
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let line = match request {
        Request::NewTab => "NewTab".to_string(),
        Request::Open(paths) => {
            let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
            format!("Open\t{}", list.join("\t"))
        }
    };
    let _ = writeln!(file, "{line}");
}

/// Run one `tabs` command and perform the actions it returns, in order. The
/// borrow of `tabs` ends before the first action: actions call Scintilla and
/// open dialogs, which send messages back here (spec §13).
fn command(a: &App, f: impl FnOnce(&mut Tabs) -> Vec<Action>) {
    let actions = f(&mut a.tabs.borrow_mut());
    if actions.is_empty() {
        return;
    }
    for action in actions {
        if a.closing.get() {
            return;
        }
        perform(a, action);
    }
    if !a.closing.get() {
        // The strip, title and status bar follow whatever changed.
        relayout(a);
        set_title(a);
        rehover(a);
        sync_rendered(a);
    }
}

fn perform(a: &App, action: Action) {
    match action {
        Action::CreateDocument(id) => {
            let doc = a.editor.create_document(false);
            adopt(a, id, doc);
        }
        Action::Load { id, path, encoding, keep_caret_line } => load(a, id, &path, encoding, keep_caret_line),
        Action::Write { id, path, encoding, eol } => files::write(a, id, &path, encoding, eol),
        Action::AskSaveAs { id, default } => files::ask_save_as(a, id, &default),
        Action::AskDiscardForReopen { id, encoding } => files::ask_discard_for_reopen(a, id, encoding),
        Action::ConvertEol { id, eol } => in_document(a, id, |e| e.convert_eols(eol)),
        Action::Activate(id) => show(a, id),
        Action::Changed(id) => {
            // 13: `command` then shows or hides the Rendered view (`sync_rendered`).
            if a.shown.get() == Some(id) {
                apply_settings(a, id);
                // The Tab may have started or stopped being styled (Save As, 1 MB).
                arm_restyle(a);
            }
            invalidate(a, None);
        }
        Action::Closed { id, document } => {
            a.views.borrow_mut().remove(&id);
            a.styled.borrow_mut().forget(id);
            if a.shown.get() == Some(id) {
                a.shown.set(None);
            }
            // The editor keeps its own hold while it still shows the document.
            if let Some(EditorDoc(doc)) = document {
                a.editor.release_document(doc);
            }
        }
        Action::AskSaveBeforeClose(id) => {
            let Some(name) = a.tabs.borrow().get(id).map(Tab::name) else { return };
            let reply = ask(a, &format!("Save changes to {name}?"), "", "Save", "Don't save");
            command(a, |t| t.close_reply(id, reply));
        }
        Action::AskSaveAll(ids) => {
            let names: Vec<String> = {
                let tabs = a.tabs.borrow();
                ids.iter().filter_map(|id| tabs.get(*id).map(Tab::name)).collect()
            };
            let reply = ask(a, "Save changes before closing?", &names.join("\n"), "Save all", "Discard all");
            command(a, |t| t.close_window_reply(reply));
        }
        // 11: watch the file's folder for Disk changes.
        Action::Watch(_) | Action::Unwatch(_) => {}
        Action::Recheck { path, after } => {
            let timer = a.next_timer.get();
            a.next_timer.set(timer + 1);
            a.rechecks.borrow_mut().push((timer, path));
            let ms = after.as_millis().clamp(1, u32::MAX as u128) as u32;
            unsafe {
                SetTimer(Some(a.hwnd), timer, ms, None);
            }
        }
        Action::AddRecent(path) => recent::add(a, &path),
        Action::OpenInAnotherTab { name } => message(a, &format!("{name} is open in another tab. Close it first.")),
        Action::Flash(note) => {
            let status = {
                let mut ui = a.ui.borrow_mut();
                ui.flash = Some(note);
                ui.layout.status
            };
            unsafe {
                SetTimer(Some(a.hwnd), FLASH_TIMER, FLASH.as_millis() as u32, None);
            }
            invalidate(a, Some(status));
        }
        Action::CloseWindow => {
            a.closing.set(true);
            unsafe {
                let _ = DestroyWindow(a.hwnd);
            }
        }
    }
}

/// Give `doc` to Tab `id`, and release whatever it replaces.
fn adopt(a: &App, id: TabId, doc: isize) {
    a.styled.borrow_mut().forget(id);
    let old = a.tabs.borrow_mut().set_document(id, EditorDoc(doc));
    if let Some(EditorDoc(old)) = old {
        a.editor.release_document(old);
    }
}

/// Keep the shown Tab's selection and scroll for when it is shown again.
fn stash_view(a: &App) {
    if let Some(id) = a.shown.get() {
        let view = a.editor.view_state();
        a.views.borrow_mut().insert(id, view);
    }
}

/// Swap Tab `id`'s document into the editor and put back its view.
fn show(a: &App, id: TabId) {
    let Some(EditorDoc(doc)) = a.tabs.borrow().get(id).and_then(Tab::document) else { return };
    stash_view(a);
    a.editor.show_document(doc);
    a.shown.set(Some(id));
    // Another document: its line ending and read-only state start afresh.
    a.applied.set(None);
    apply_settings(a, id);
    let view = a.views.borrow().get(&id).copied().unwrap_or_default();
    a.editor.restore_view(&view);
    restyle(a);
    find::on_show(a);
    refresh_status(a);
}

/// Style (or unstyle) the shown Tab to match `Tab::styled`, if it isn't already.
fn restyle(a: &App) {
    let Some(id) = a.shown.get() else { return };
    let wanted = a.tabs.borrow().get(id).is_some_and(Tab::styled);
    let plan = a.styled.borrow_mut().plan(id, wanted);
    mdview::run(&a.editor, plan);
    arm_spell(a);
}

/// Spell check the shown Tab once things are quiet, after an edit, a scroll or a switch.
fn arm_spell(a: &App) {
    unsafe {
        SetTimer(Some(a.hwnd), TIMER_SPELL, SPELL_MS, None);
    }
}

/// Squiggle the shown Tab's flagged words, or clear them if it isn't spell checked.
fn spellcheck(a: &App) {
    let Some(id) = a.shown.get() else { return };
    let wanted = a.tabs.borrow().get(id).is_some_and(Tab::spell_checked);
    a.spell.run(&a.editor, wanted);
}

/// Restyle once typing pauses; each edit restarts the wait.
fn arm_restyle(a: &App) {
    unsafe {
        SetTimer(Some(a.hwnd), TIMER_RESTYLE, RESTYLE_MS, None);
    }
}

/// The shown Tab's line ending, read-only state and wrap. Only a change is
/// sent: `Changed` can arrive from inside Scintilla's own SCN_MODIFIED.
fn apply_settings(a: &App, id: TabId) {
    let Some(settings) = a.tabs.borrow().get(id).map(|t| (t.eol(), t.read_only(), t.wrap_allowed())) else {
        return;
    };
    if a.applied.replace(Some(settings)) == Some(settings) {
        return;
    }
    let (eol, read_only, wrap) = settings;
    a.editor.set_eol_mode(eol);
    a.editor.set_read_only(read_only);
    a.editor.set_wrap(wrap && findview::wrap_wanted());
}

/// Run `f` on `doc` in the editor, then show the shown Tab again.
fn with_document(a: &App, doc: isize, f: impl FnOnce(&Editor)) {
    stash_view(a);
    let shown = a.shown.take();
    quietly(a, |e| {
        e.show_document(doc);
        f(e);
    });
    if let Some(id) = shown {
        show(a, id);
    }
}

/// Run `f`, whose text changes are scrivi's own, not edits.
fn quietly(a: &App, f: impl FnOnce(&Editor)) {
    a.quiet.set(true);
    f(&a.editor);
    a.quiet.set(false);
}

/// Run `f` on Tab `id`'s document, shown or not.
fn in_document(a: &App, id: TabId, f: impl FnOnce(&Editor)) {
    if a.shown.get() == Some(id) {
        return quietly(a, f);
    }
    let Some(EditorDoc(doc)) = a.tabs.borrow().get(id).and_then(Tab::document) else { return };
    with_document(a, doc, f);
}

/// `Action::Load`: read and decode the file into a new document for the Tab.
fn load(a: &App, id: TabId, path: &Path, encoding: Option<Encoding>, keep_caret_line: bool) {
    let (bytes, stat) = match read_file(path) {
        Ok(read) => read,
        Err(e) => {
            // A reread that fails keeps the text; only a Tab that never showed it says why.
            let first = a.tabs.borrow().get(id).is_some_and(|t| t.document().is_none());
            // The outcome goes in before the message box pumps messages.
            command(a, |t| t.load_failed(id));
            if first {
                message(a, &format!("Couldn't open {}.\n\n{e}", path.display()));
            }
            return;
        }
    };
    let decoded = match encoding {
        Some(encoding) => {
            let decoded = textfile::decode_as(&bytes, encoding);
            drop(bytes);
            decoded
        }
        None => textfile::decode_owned(bytes, unsafe { GetACP() } as u16),
    };
    let mut view = ViewState::default();
    if keep_caret_line {
        if a.shown.get() == Some(id) {
            stash_view(a);
        }
        view = a.views.borrow().get(&id).copied().unwrap_or_default();
    }
    let large = textfile::is_large(stat.size);
    let doc = a.editor.create_document(large);
    with_document(a, doc, |e| {
        e.set_eol_mode(decoded.eol);
        e.fill(&decoded.text, large);
        if keep_caret_line {
            let start = e.line_start(view.caret_line);
            view = ViewState { anchor: start, caret: start, x_offset: 0, ..view };
        }
    });
    adopt(a, id, doc);
    a.views.borrow_mut().insert(id, view);
    if a.shown.get() == Some(id) {
        // The old document is gone: show the new one without keeping its view.
        a.shown.set(None);
        show(a, id);
    }
    let (encoding, eol, lossy) = (decoded.encoding, decoded.eol, decoded.lossy);
    let file_id = files::file_id(path);
    let file = LoadedFile { encoding, eol, stat, file_id, text_len: decoded.text.len(), lossy };
    command(a, |t| t.loaded(id, file));
    // The Tab now knows its size: style it, or leave it plain over 1 MB.
    restyle(a);
}

fn read_file(path: &Path) -> std::io::Result<(Vec<u8>, FileStat)> {
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok((bytes, FileStat { size: meta.len(), modified: meta.modified()? }))
}

/// `path` on disk now, for `Tabs::disk_event`; `None` if it is gone.
fn stat_of(path: &Path) -> Option<FileStat> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStat { size: meta.len(), modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
}

/// A two-choice question with Cancel (spec §8): `save` / `discard` / Cancel.
/// Pumps messages, so nothing may be borrowed across it.
fn ask(a: &App, instruction: &str, content: &str, save: &str, discard: &str) -> CloseReply {
    const SAVE: i32 = 100;
    const DISCARD: i32 = 101;
    if headless() {
        return CloseReply::Cancel;
    }
    let (instruction, content, save, discard) = (wide(instruction), wide(content), wide(save), wide(discard));
    let buttons = [
        TASKDIALOG_BUTTON { nButtonID: SAVE, pszButtonText: PCWSTR(save.as_ptr()) },
        TASKDIALOG_BUTTON { nButtonID: DISCARD, pszButtonText: PCWSTR(discard.as_ptr()) },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: a.hwnd,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: w!("scrivi"),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: if content.len() > 1 { PCWSTR(content.as_ptr()) } else { PCWSTR::null() },
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: SAVE,
        ..Default::default()
    };
    let mut pressed = 0;
    if unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) }.is_err() {
        return CloseReply::Cancel;
    }
    match pressed {
        SAVE => CloseReply::Save,
        DISCARD => CloseReply::Discard,
        _ => CloseReply::Cancel,
    }
}

/// A plain message with OK. Pumps messages, so nothing may be borrowed across it.
fn message(a: &App, text: &str) {
    if headless() {
        return;
    }
    let text = wide(text);
    unsafe {
        MessageBoxW(Some(a.hwnd), PCWSTR(text.as_ptr()), w!("scrivi"), MB_OK | MB_ICONINFORMATION);
    }
}

/// For Alt+Tab and the taskbar: there is no title text in the row.
fn set_title(a: &App) {
    let title = {
        let tabs = a.tabs.borrow();
        match tabs.active().and_then(|id| tabs.get(id)) {
            Some(tab) => format!("{} \u{2014} scrivi", tab.name()),
            None => "scrivi".to_string(),
        }
    };
    {
        let mut ui = a.ui.borrow_mut();
        if ui.title == title {
            return;
        }
        ui.title = title.clone();
    }
    let title = wide(&title);
    unsafe {
        let _ = SetWindowTextW(a.hwnd, PCWSTR(title.as_ptr()));
    }
}

/// Re-read the caret and zoom; repaint the status bar if they changed.
fn refresh_status(a: &App) {
    let caret = a.editor.caret();
    let zoom = a.editor.zoom_percent();
    let digits = a.editor.line_digits();
    let (changed, refit, status) = {
        let mut ui = a.ui.borrow_mut();
        let changed = (ui.caret, ui.zoom) != (caret, zoom);
        // Zoom scales the line numbers too.
        let refit = ui.digits != digits || ui.zoom != zoom;
        ui.caret = caret;
        ui.zoom = zoom;
        ui.digits = digits;
        (changed, refit, ui.layout.status)
    };
    if refit {
        a.editor.fit_line_numbers(unsafe { GetDpiForWindow(a.hwnd) });
    }
    if changed {
        invalidate(a, Some(status));
    }
}

fn status_of(ui: &Ui, tabs: &Tabs) -> Status {
    let tab = tabs.active().and_then(|id| tabs.get(id));
    let (encoding, eol) = tab.map_or(textfile::NEW_FILE, |t| (t.encoding(), t.eol()));
    let mut status = Status::source(ui.caret.line, ui.caret.col, eol.name(), encoding.name(), ui.zoom);
    if let Some(note) = ui.flash.or_else(|| tab.and_then(Tab::status_note)) {
        status.note = note.text().to_string();
    }
    status
}

fn invalidate(a: &App, r: Option<Rect>) {
    let rect = r.map(|r| RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
    unsafe {
        let _ = InvalidateRect(Some(a.hwnd), rect.as_ref().map(|r| r as *const RECT), false);
    }
}

/// The id of the Tab at strip position `i`.
fn tab_at(a: &App, i: usize) -> Option<TabId> {
    a.tabs.borrow().iter().nth(i).map(Tab::id)
}

/// Recompute the layout from the client size and the Tabs, scroll a newly
/// active Tab into view, and move the editor into its band.
fn relayout(a: &App) {
    let hwnd = a.hwnd;
    let mut rc = RECT::default();
    let (dpi, maximized) = unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        (GetDpiForWindow(hwnd), IsZoomed(hwnd).as_bool())
    };
    let border = resize_border(dpi).1;
    let (count, active, active_at, notice_h) = {
        let tabs = a.tabs.borrow();
        let active = tabs.active();
        let at = tabs.iter().position(|t| Some(t.id()) == active);
        let notice_h = if notice::spec(&tabs).is_some() { crate::chrome::notice::HEIGHT } else { 0.0 };
        (tabs.iter().count(), active, at, notice_h)
    };
    let editor = {
        let mut ui = a.ui.borrow_mut();
        let frame = Frame {
            width: rc.right,
            height: rc.bottom,
            scale: dpi as f32 / 96.0,
            notice_h,
            tabs: count,
            tab_scroll: ui.tab_scroll,
            menu_text: ui.menu_text,
            maximized,
            resize_border: border,
            find_h: find::band_height(),
        };
        let mut layout = Layout::new(&frame);
        // A newly active Tab, or a new width, scrolls the active Tab into view;
        // only ‹ › may leave it out.
        if ui.revealed != active || ui.layout.status.right != rc.right {
            ui.revealed = active;
            if let Some(i) = active_at {
                layout = Layout::new(&Frame { tab_scroll: layout.reveal(i), ..frame });
            }
        }
        ui.tab_scroll = layout.tab_scroll;
        ui.layout = layout;
        ui.layout.editor
    };
    a.editor.move_to(editor);
    webview::set_bounds(rect_of(editor));
    find::place(a);
    invalidate(a, None);
}

/// After the strip changed under a still mouse: light up what it is over now.
fn rehover(a: &App) {
    if !a.ui.borrow().tracking {
        return;
    }
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let _ = ScreenToClient(a.hwnd, &mut pt);
    }
    let hit = a.ui.borrow().layout.hit_test(pt.x, pt.y);
    set_hover(a, Some(hit).filter(|h| h.lights_up()));
}

/// The mouse went down on a button we draw: show it pressed and capture the
/// mouse until it comes up.
fn press(a: &App, hit: Hit) {
    let title = {
        let mut ui = a.ui.borrow_mut();
        ui.pressed = Some(hit);
        ui.hover = Some(hit);
        ui.layout.title
    };
    invalidate(a, Some(title));
    unsafe {
        SetCapture(a.hwnd);
    }
}

/// The mouse came up: end the press, and return what was pressed.
fn release(a: &App) -> Option<Hit> {
    let (pressed, title) = {
        let mut ui = a.ui.borrow_mut();
        (ui.pressed.take(), ui.layout.title)
    };
    invalidate(a, Some(title));
    unsafe {
        if GetCapture() == a.hwnd {
            // Sends WM_CAPTURECHANGED: nothing may be borrowed here.
            let _ = ReleaseCapture();
        }
    }
    pressed
}

/// A press and release on the same button.
fn click(a: &App, hit: Hit) {
    let hwnd = a.hwnd;
    let command_id = match hit {
        Hit::Minimize => SC_MINIMIZE,
        Hit::Maximize if unsafe { IsZoomed(hwnd) }.as_bool() => SC_RESTORE,
        Hit::Maximize => SC_MAXIMIZE,
        Hit::Close => SC_CLOSE,
        Hit::TabClose(i) => {
            if let Some(id) = tab_at(a, i) {
                command(a, |t| t.close(id));
            }
            return;
        }
        Hit::NewTab => return command(a, Tabs::new_untitled),
        Hit::ScrollLeft | Hit::ScrollRight => {
            {
                let mut ui = a.ui.borrow_mut();
                ui.tab_scroll = ui.layout.scroll_step(if hit == Hit::ScrollLeft { -1 } else { 1 });
            }
            relayout(a);
            return rehover(a);
        }
        _ => return,
    };
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(command_id as usize), LPARAM(0));
    }
}

fn set_hover(a: &App, hover: Option<Hit>) {
    let title = {
        let mut ui = a.ui.borrow_mut();
        if ui.hover == hover {
            return;
        }
        ui.hover = hover;
        ui.layout.title
    };
    invalidate(a, Some(title));
    update_tip(a);
}

/// Point the tooltip at the Tab under the mouse: its full path, or "Untitled N".
fn update_tip(a: &App) {
    let (tip, rect) = {
        let ui = a.ui.borrow();
        let tip = ui.hover.and_then(Hit::tab).and_then(|i| tab_at(a, i).map(|id| (id, i)));
        let rect = tip.map_or(RECT::default(), |(_, i)| {
            let (t, s) = (ui.layout.tabs[i], ui.layout.strip);
            RECT { left: t.left.max(s.left), top: t.top, right: t.right.min(s.right), bottom: t.bottom }
        });
        (tip.map(|(id, _)| id), rect)
    };
    if a.ui.borrow().tip == tip {
        return;
    }
    a.ui.borrow_mut().tip = tip;
    let text = tip
        .and_then(|id| {
            let tabs = a.tabs.borrow();
            tabs.get(id).map(|t| t.path().map_or_else(|| t.name(), |p| p.display().to_string()))
        })
        .unwrap_or_default();
    let mut text = wide(&text);
    let info = TTTOOLINFOW {
        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
        hwnd: a.hwnd,
        uId: TIP_TOOL,
        rect,
        lpszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    let info = Some(LPARAM(&info as *const _ as isize));
    unsafe {
        SendMessageW(a.tooltip, TTM_POP, None, None);
        SendMessageW(a.tooltip, TTM_NEWTOOLRECTW, None, info);
        SendMessageW(a.tooltip, TTM_UPDATETIPTEXTW, None, info);
    }
}

fn track_leave(a: &App, nonclient: bool) {
    let already = {
        let mut ui = a.ui.borrow_mut();
        let flag = if nonclient { &mut ui.tracking_nc } else { &mut ui.tracking };
        std::mem::replace(flag, true)
    };
    if already {
        return;
    }
    let mut tme = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: if nonclient { TME_LEAVE | TME_NONCLIENT } else { TME_LEAVE },
        hwndTrack: a.hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
}

fn paint(a: &App) {
    let painted = unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(a.hwnd, &mut ps);
        let find = find::view(a);
        let painted = {
            let ui = a.ui.borrow();
            let tabs = a.tabs.borrow();
            let status = status_of(&ui, &tabs);
            let names: Vec<(String, bool, bool)> =
                tabs.iter().map(|t| (t.name(), Some(t.id()) == tabs.active(), t.unsaved())).collect();
            let strip: Vec<TabView> =
                names.iter().map(|(name, active, unsaved)| TabView { name, active: *active, unsaved: *unsaved }).collect();
            let (menu_focus, access_keys) = menu::painted();
            let notice = notice::spec(&tabs);
            let view = View {
                notice: notice.as_ref(),
                layout: &ui.layout,
                menu_focus,
                access_keys,
                tabs: &strip,
                status: &status,
                hover: ui.hover,
                // Dragged off its button, a press no longer shows.
                pressed: ui.pressed.filter(|p| ui.hover == Some(*p)),
                active: ui.active,
                find: find.as_ref(),
            };
            a.painter.borrow_mut().paint(hdc, &ps.rcPaint, &view)
        };
        let _ = EndPaint(a.hwnd, &ps);
        painted
    };
    // A lost target (D2DERR_RECREATE_TARGET, ...) is rebuilt by the next
    // paint: ask for one, a few times at most.
    let retry = {
        let mut ui = a.ui.borrow_mut();
        ui.paint_failures = if painted.is_ok() { 0 } else { ui.paint_failures + 1 };
        (painted.is_err() && ui.paint_failures <= PAINT_RETRIES).then(|| {
            let l = &ui.layout;
            [Rect { bottom: l.editor.top, ..l.title }, l.status]
        })
    };
    for band in retry.into_iter().flatten() {
        invalidate(a, Some(band));
    }
}

/// `WM_NCCALCSIZE`: the client takes the native caption's place; the side and
/// bottom borders stay, so resizing and the shadow work as usual. Maximised,
/// the Window overhangs the screen by the border, so the top is inset too.
unsafe fn calc_size(hwnd: HWND, lp: LPARAM) -> LRESULT {
    unsafe {
        let params = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
        let r = &mut params.rgrc[0];
        let (bx, by) = resize_border(GetDpiForWindow(hwnd));
        r.left += bx;
        r.right -= bx;
        r.bottom -= by;
        if IsZoomed(hwnd).as_bool() {
            r.top += by;
            leave_room_for_autohide_bars(hwnd, r);
        }
    }
    LRESULT(0)
}

/// A maximised Window covering the whole monitor hides an auto-hide taskbar
/// for good: leave it a 1 px strip on its edge, as Chromium does.
unsafe fn leave_room_for_autohide_bars(hwnd: HWND, r: &mut RECT) {
    unsafe {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            return;
        }
        for edge in [ABE_LEFT, ABE_TOP, ABE_RIGHT, ABE_BOTTOM] {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                uEdge: edge,
                rc: mi.rcMonitor,
                ..Default::default()
            };
            if SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &mut abd) == 0 {
                continue;
            }
            match edge {
                ABE_LEFT => r.left += 1,
                ABE_TOP => r.top += 1,
                ABE_RIGHT => r.right -= 1,
                _ => r.bottom -= 1,
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCALCSIZE && wp.0 != 0 {
            return calc_size(hwnd, lp);
        }
        let Some(a) = app() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_NCHITTEST => {
                let (x, y) = xy(lp);
                let mut pt = POINT { x, y };
                let _ = ScreenToClient(hwnd, &mut pt);
                let hit = a.ui.borrow().layout.hit_test(pt.x, pt.y);
                LRESULT(hit.nc_code() as isize)
            }
            // Over our caption buttons Windows reports non-client mouse
            // messages; we draw and click them ourselves so DefWindowProc
            // never paints classic buttons over them.
            WM_NCMOUSEMOVE => {
                track_leave(a, true);
                set_hover(a, Hit::caption_button(wp.0 as u32));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCMOUSELEAVE => {
                let title = {
                    let mut ui = a.ui.borrow_mut();
                    ui.tracking_nc = false;
                    // A press holds the capture: it keeps its hover until released.
                    if ui.pressed.is_none() {
                        ui.hover = ui.hover.filter(|h| Hit::caption_button(h.nc_code()).is_none());
                    }
                    ui.layout.title
                };
                invalidate(a, Some(title));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => match Hit::caption_button(wp.0 as u32) {
                Some(button) => {
                    press(a, button);
                    LRESULT(0)
                }
                None => DefWindowProcW(hwnd, msg, wp, lp),
            },
            // The press captured the mouse, so its release normally arrives as
            // WM_LBUTTONUP; this covers a capture lost on the way.
            WM_NCLBUTTONUP => {
                let pressed = release(a);
                match Hit::caption_button(wp.0 as u32) {
                    Some(button) => {
                        if pressed == Some(button) {
                            click(a, button);
                        }
                        LRESULT(0)
                    }
                    None => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                let hit = a.ui.borrow().layout.hit_test(x, y);
                match hit {
                    // Tabs activate on the way down, as Windows' own do.
                    Hit::Tab(i) => {
                        if let Some(id) = tab_at(a, i) {
                            command(a, |t| t.activate(id));
                        }
                    }
                    Hit::TabClose(_) | Hit::NewTab | Hit::ScrollLeft | Hit::ScrollRight => press(a, hit),
                    // A native menu bar opens on the way down.
                    Hit::Menu(i) => menu::open(a, i, false),
                    Hit::Find => find::click(a, x, y),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                let hit = a.ui.borrow().layout.hit_test(x, y);
                if release(a) == Some(hit) {
                    click(a, hit);
                }
                if hit == Hit::Notice {
                    notice::click(a, x, y);
                }
                LRESULT(0)
            }
            WM_MBUTTONDOWN | WM_MBUTTONUP => {
                let (x, y) = xy(lp);
                let hit = a.ui.borrow().layout.hit_test(x, y);
                let under = hit.tab().and_then(|i| tab_at(a, i));
                if msg == WM_MBUTTONDOWN {
                    a.ui.borrow_mut().middle = under;
                    return LRESULT(0);
                }
                let down = a.ui.borrow_mut().middle.take();
                // Middle-click closes a Tab.
                if let Some(id) = under.filter(|id| down == Some(*id)) {
                    command(a, |t| t.close(id));
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                let title = {
                    let mut ui = a.ui.borrow_mut();
                    ui.pressed = None;
                    ui.layout.title
                };
                invalidate(a, Some(title));
                LRESULT(0)
            }
            WM_CANCELMODE => {
                if GetCapture() == hwnd {
                    let _ = ReleaseCapture();
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOUSEMOVE => {
                track_leave(a, false);
                let (x, y) = xy(lp);
                let hit = a.ui.borrow().layout.hit_test(x, y);
                set_hover(a, Some(hit).filter(|h| h.lights_up()));
                if hit == Hit::Find {
                    find::hover(a, x, y);
                } else {
                    find::leave(a);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                a.ui.borrow_mut().tracking = false;
                set_hover(a, None);
                find::leave(a);
                LRESULT(0)
            }
            WM_COMMAND => {
                // The find bar's inputs tell us about typing and focus.
                if find::control_notify(a, wp, lp) {
                    return LRESULT(0);
                }
                match (wp.0 & 0xffff) as u16 {
                    cmd::NEW_TAB => command(a, Tabs::new_untitled),
                    cmd::CLOSE_TAB => command(a, |t| t.active().map_or_else(Vec::new, |id| t.close(id))),
                    cmd::NEXT_TAB => command(a, Tabs::next),
                    cmd::PREV_TAB => command(a, Tabs::prev),
                    cmd::RENDERED_VIEW => command(a, |t| t.active().map_or_else(Vec::new, |id| t.toggle_view(id))),
                    cmd::SHOW_SOURCE => command(a, |t| t.active().map_or_else(Vec::new, |id| t.show_source(id))),
                    id => {
                        if !menu::run(a, id) {
                            return DefWindowProcW(hwnd, msg, wp, lp);
                        }
                    }
                }
                LRESULT(0)
            }
            // Closing the Window asks about unsaved Tabs first (spec §8).
            WM_CLOSE => {
                command(a, Tabs::close_window);
                LRESULT(0)
            }
            WM_TIMER => {
                let timer = wp.0;
                let _ = KillTimer(Some(hwnd), timer);
                if timer == TIMER_RESTYLE {
                    restyle(a);
                } else if timer == TIMER_PREFS {
                    recent::flush(hwnd);
                } else if timer == TIMER_SPELL {
                    spellcheck(a);
                } else if timer == proto::TIMER_FIND {
                    find::timer(a);
                } else if timer == FLASH_TIMER {
                    let status = {
                        let mut ui = a.ui.borrow_mut();
                        ui.flash = None;
                        ui.layout.status
                    };
                    invalidate(a, Some(status));
                } else {
                    let path = {
                        let mut rechecks = a.rechecks.borrow_mut();
                        rechecks.iter().position(|(t, _)| *t == timer).map(|i| rechecks.remove(i).1)
                    };
                    if let Some(path) = path {
                        let on_disk = stat_of(&path);
                        command(a, |t| t.disk_event(&path, on_disk, Instant::now()));
                    }
                }
                LRESULT(0)
            }
            WM_SIZE => {
                // Minimised, the client is 0 x 0: resizing Scintilla to it
                // would rewrap the text and lose the scroll position.
                if wp.0 as u32 != SIZE_MINIMIZED {
                    relayout(a);
                }
                recent::window_changed(hwnd);
                webview::set_minimized(wp.0 as u32 == SIZE_MINIMIZED);
                if wp.0 as u32 != SIZE_MINIMIZED {
                    rendered_view::refocus(a);
                }
                LRESULT(0)
            }
            WM_MOVE => {
                recent::window_changed(hwnd);
                webview::parent_moved();
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                let s = scale_of(hwnd);
                info.ptMinTrackSize = POINT { x: (MIN_W * s) as i32, y: (MIN_H * s) as i32 };
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let _ = a.painter.borrow_mut().set_scale((wp.0 & 0xffff) as f32 / 96.0);
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                // Scintilla learns the DPI from WM_DPICHANGED_AFTERPARENT, after this returns.
                let _ = PostMessageW(Some(hwnd), WM_EDITOR_DPI, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            WM_EDITOR_DPI => {
                a.editor.apply_dpi(GetDpiForWindow(hwnd));
                relayout(a);
                LRESULT(0)
            }
            WM_PAINT => {
                paint(a);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_ACTIVATE => {
                let title = {
                    let mut ui = a.ui.borrow_mut();
                    ui.active = (wp.0 & 0xffff) as u32 != WA_INACTIVE;
                    ui.layout.title
                };
                invalidate(a, Some(title));
                if (wp.0 & 0xffff) as u32 == WA_INACTIVE {
                    menu::leave(a);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            // -1: activate without repainting a native caption we no longer have.
            WM_NCACTIVATE => DefWindowProcW(hwnd, msg, wp, LPARAM(-1)),
            WM_SETFOCUS => {
                // 13: in the Rendered view the page takes the focus.
                if webview::focus() {
                    return LRESULT(0);
                }
                let _ = SetFocus(Some(a.editor.hwnd()));
                LRESULT(0)
            }
            WM_NOTIFY => {
                let n = &*(lp.0 as *const NotifyHeader);
                if n.hwnd_from == a.editor.hwnd().0 {
                    match n.code {
                        // Undo back to the save point clears the unsaved dot.
                        sci::SCN_SAVEPOINTREACHED | sci::SCN_SAVEPOINTLEFT if !a.quiet.get() => {
                            files::save_point(a, n.code == sci::SCN_SAVEPOINTLEFT)
                        }
                        sci::SCN_UPDATEUI | sci::SCN_ZOOM => {
                            if n.code == sci::SCN_ZOOM {
                                recent::zoom_changed(a);
                            }
                            refresh_status(a);
                            if a.spell.scrolled_away(&a.editor) {
                                arm_spell(a);
                            }
                        }
                        // Text typed, deleted, undone: the shown Tab is edited.
                        sci::SCN_MODIFIED if !a.quiet.get() => {
                            if let Some(id) = a.shown.get() {
                                let len = a.editor.text_len();
                                command(a, |t| t.edited(id, len));
                                a.styled.borrow_mut().touch(id);
                                arm_restyle(a);
                                find::edited(a);
                            }
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            // The editor's own popup is off: right-click shows spell check's menu.
            WM_CONTEXTMENU if wp.0 == a.editor.hwnd().0 as usize => {
                // 13: no spell menu over the source behind the Rendered view.
                if rendered_view::behind_page() {
                    return LRESULT(0);
                }
                let wanted = a.shown.get().is_some_and(|id| a.tabs.borrow().get(id).is_some_and(Tab::spell_checked));
                a.spell.context_menu(&a.editor, hwnd, lp, wanted);
                LRESULT(0)
            }
            WM_COPYDATA => {
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData != COPYDATA_REQUEST {
                    return LRESULT(0);
                }
                let units = cds.cbData as usize / 2;
                let payload: &[u16] = if units == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(cds.lpData as *const u16, units)
                };
                // Answer the waiting launch at once; files load (and any
                // dialog shows) once it has gone.
                let request = Box::new(launch::decode(payload));
                let _ = PostMessageW(Some(hwnd), WM_HANDOFF, WPARAM(0), LPARAM(Box::into_raw(request) as isize));
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
                LRESULT(1)
            }
            WM_HANDOFF => {
                let request = Box::from_raw(lp.0 as *mut Request);
                on_handoff(*request);
                LRESULT(0)
            }
            // Alt or F10 alone, and Alt+F, E or V: the menu bar (SCRIVI-16).
            WM_SYSCOMMAND if (wp.0 & 0xfff0) as u32 == SC_KEYMENU => {
                if menu::key_menu(a, lp.0 as u32) {
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
            }
            WM_ENTERMENULOOP => {
                menu::loop_entered(a);
                LRESULT(0)
            }
            WM_MENUSELECT => {
                menu::selected(wp, lp);
                LRESULT(0)
            }
            WM_DROPFILES => {
                files::dropped(a, windows::Win32::UI::Shell::HDROP(wp.0 as *mut _));
                LRESULT(0)
            }
            WM_RENDERED_SHOWN | WM_RENDERED_LINK | WM_RENDERED_FAILED => on_rendered(a, msg, wp, lp),
            WM_DESTROY => {
                recent::flush(hwnd);
                webview::close();
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod cmd_tests {
    use super::cmd::*;
    use crate::textfile::{Encoding, Eol};

    #[test]
    fn every_command_id_and_menu_range_is_distinct() {
        let mut ids = vec![
            NEW_TAB, CLOSE_TAB, NEXT_TAB, PREV_TAB, OPEN, SAVE, SAVE_AS, EXIT, UNDO, REDO, CUT, COPY, PASTE,
            SELECT_ALL, FIND, REPLACE, GO_TO_LINE, ENCODING_KEEP, REOPEN, RENDERED_VIEW, WORD_WRAP, ZOOM_IN,
            ZOOM_OUT, ZOOM_RESET, SHOW_SOURCE,
        ];
        ids.extend((0..Encoding::MENU.len() as u16).map(|i| ENCODING + i));
        ids.extend((0..Eol::MENU.len() as u16).map(|i| EOL + i));
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "{ids:?}");
    }
}
