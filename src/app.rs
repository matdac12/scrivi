//! The Window, its wndproc, the message loop and the glue between modules.
//!
//! The Window draws its own one-row title bar (ADR 0002): `WM_NCCALCSIZE`
//! drops the native caption but keeps the left, right and bottom resize
//! borders (and so the shadow); `WM_NCHITTEST` answers from the pure
//! `chrome::Layout::hit_test`. One Scintilla control fills the editor band.
//!
//! Debug builds append each request to the file named by `SCRIVI_TEST_LOG`,
//! if set, and keep the Window hidden then, so the launch tests stay headless.

use std::cell::{OnceCell, RefCell};
#[cfg(debug_assertions)]
use std::io::Write;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, ScreenToClient, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::chrome::{Frame, Hit, Layout, Painter, Rect, Status, TabView, View};
use crate::editor::{sci, Caret, Editor, NotifyHeader};
use crate::launch::{self, Request};
use crate::proto::{CLASS, COPYDATA_REQUEST, ICON, WM_EDITOR_DPI};
use crate::textfile::{Encoding, Eol};

/// Not exported by the `windows` features we enable (it lives in UI_Controls).
const WM_MOUSELEAVE: u32 = 0x02A3;
/// First size of the Window, in DIPs, before prefs (09) remembers one.
const START_W: f32 = 1100.0;
const START_H: f32 = 760.0;
const MIN_W: f32 = 480.0;
const MIN_H: f32 = 240.0;
/// Repaints asked for after failed paints, before giving up until the next
/// ordinary one.
const PAINT_RETRIES: u32 = 3;

/// What the chrome shows; borrowed only for short stretches that make no
/// Win32 call which could send a message back (spec §13).
struct Ui {
    layout: Layout,
    menu_text: [f32; 3],
    /// The one Tab until 07: its name, encoding and line ending.
    tab_name: String,
    encoding: Encoding,
    eol: Eol,
    caret: Caret,
    zoom: i32,
    digits: u32,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    tracking: bool,
    tracking_nc: bool,
    active: bool,
    /// Paints failed in a row, for `PAINT_RETRIES`.
    paint_failures: u32,
}

struct App {
    hwnd: HWND,
    editor: Editor,
    painter: RefCell<Painter>,
    ui: RefCell<Ui>,
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

/// Debug builds started by the launch tests stay hidden.
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
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return 1;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
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
        place(hwnd);

        let dpi = GetDpiForWindow(hwnd);
        let Ok(painter) = Painter::new(hinstance, dpi as f32 / 96.0) else { return 1 };
        let Some(editor) = Editor::create(hwnd, hinstance, dpi) else { return 1 };
        let menu_text = painter.menu_text();
        let ui = Ui {
            layout: Layout::new(&Frame::default()),
            menu_text,
            tab_name: String::new(),
            encoding: Encoding::default(),
            eol: Eol::default(),
            caret: Caret::default(),
            zoom: 100,
            digits: 0,
            hover: None,
            pressed: None,
            tracking: false,
            tracking_nc: false,
            active: true,
            paint_failures: 0,
        };
        let a: &'static App = Box::leak(Box::new(App { hwnd, editor, painter: RefCell::new(painter), ui: RefCell::new(ui) }));
        APP.with(|c| {
            let _ = c.set(a);
        });
        // The start state: one empty Untitled tab, unless the launch names a file.
        set_tab(a, "Untitled 1", (Encoding::default(), Eol::default()));
        relayout(a);
        on_handoff(initial);
        if !headless() {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetFocus(Some(a.editor.hwnd()));
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
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

/// A request from this launch (`run`'s `initial`) or a later one (`WM_COPYDATA`).
pub fn on_handoff(request: Request) {
    // Test hook, debug builds only.
    #[cfg(debug_assertions)]
    log_request(&request);
    let Some(a) = app() else { return };
    match request {
        // One Tab until 07: a later "new tab" only brings the Window forward.
        Request::NewTab => {}
        // Until 07 the one Tab shows the first file, unless it holds
        // unsaved edits: those are never dropped.
        Request::Open(paths) => {
            if let Some(path) = paths.first().filter(|_| !a.editor.is_modified()) {
                open(a, path);
            }
        }
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

/// Show `path` in the one Tab. A file that can't be read opens empty under its name.
fn open(a: &App, path: &Path) {
    let bytes = std::fs::read(path).unwrap_or_default();
    let found = a.editor.load_utf8(&bytes);
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    set_tab(a, &name, found);
}

fn set_tab(a: &App, name: &str, (encoding, eol): (Encoding, Eol)) {
    {
        let mut ui = a.ui.borrow_mut();
        ui.tab_name = name.to_string();
        ui.encoding = encoding;
        ui.eol = eol;
    }
    // For Alt+Tab and the taskbar: there is no title text in the row.
    let title = wide(&format!("{name} \u{2014} scrivi"));
    unsafe {
        let _ = SetWindowTextW(a.hwnd, PCWSTR(title.as_ptr()));
    }
    refresh_status(a);
    invalidate(a, None);
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

fn status_of(ui: &Ui) -> Status {
    Status::source(ui.caret.line, ui.caret.col, ui.eol.name(), ui.encoding.name(), ui.zoom)
}

fn invalidate(a: &App, r: Option<Rect>) {
    let rect = r.map(|r| RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
    unsafe {
        let _ = InvalidateRect(Some(a.hwnd), rect.as_ref().map(|r| r as *const RECT), false);
    }
}

/// Recompute the layout from the client size and move the editor into its band.
fn relayout(a: &App) {
    let hwnd = a.hwnd;
    let mut rc = RECT::default();
    let (dpi, maximized) = unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        (GetDpiForWindow(hwnd), IsZoomed(hwnd).as_bool())
    };
    let border = resize_border(dpi).1;
    let editor = {
        let mut ui = a.ui.borrow_mut();
        let frame = Frame {
            width: rc.right,
            height: rc.bottom,
            scale: dpi as f32 / 96.0,
            menu_text: ui.menu_text,
            maximized,
            resize_border: border,
            ..Frame::default()
        };
        ui.layout = Layout::new(&frame);
        ui.layout.editor
    };
    a.editor.move_to(editor);
    invalidate(a, None);
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
    let command = match hit {
        Hit::Minimize => SC_MINIMIZE,
        Hit::Maximize if unsafe { IsZoomed(hwnd) }.as_bool() => SC_RESTORE,
        Hit::Maximize => SC_MAXIMIZE,
        Hit::Close => SC_CLOSE,
        _ => return,
    };
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0));
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
        let painted = {
            let ui = a.ui.borrow();
            let status = status_of(&ui);
            let tabs = [TabView { name: &ui.tab_name, active: true, unsaved: false }];
            let view = View {
                layout: &ui.layout,
                tabs: &tabs,
                status: &status,
                hover: ui.hover,
                // Dragged off its button, a press no longer shows.
                pressed: ui.pressed.filter(|p| ui.hover == Some(*p)),
                active: ui.active,
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
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                let hit = a.ui.borrow().layout.hit_test(x, y);
                if release(a) == Some(hit) {
                    click(a, hit);
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
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                a.ui.borrow_mut().tracking = false;
                set_hover(a, None);
                LRESULT(0)
            }
            WM_SIZE => {
                // Minimised, the client is 0 x 0: resizing Scintilla to it
                // would rewrap the text and lose the scroll position.
                if wp.0 as u32 != SIZE_MINIMIZED {
                    relayout(a);
                }
                LRESULT(0)
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
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            // -1: activate without repainting a native caption we no longer have.
            WM_NCACTIVATE => DefWindowProcW(hwnd, msg, wp, LPARAM(-1)),
            WM_SETFOCUS => {
                let _ = SetFocus(Some(a.editor.hwnd()));
                LRESULT(0)
            }
            WM_NOTIFY => {
                let n = &*(lp.0 as *const NotifyHeader);
                if n.hwnd_from == a.editor.hwnd().0 && matches!(n.code, sci::SCN_UPDATEUI | sci::SCN_ZOOM) {
                    refresh_status(a);
                }
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
                on_handoff(launch::decode(payload));
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
                LRESULT(1)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
