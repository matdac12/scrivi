//! The Rendered view's WebView2: created when a Tab first shows it, closed
//! when no Tab is in it (spec §6).
//!
//! Creation is asynchronous on the UI thread, with no nested message pump: a
//! small state machine, `Off → Creating → Ready`, that each completion
//! handler moves on. `close` bumps a generation number, so a completion that
//! arrives after it (the Window closing, or every Tab back in the Source
//! view) is dropped, and a controller born late is closed at once.
//!
//! Handlers never capture the controller or the webview (that would make a
//! reference cycle `Close` exists to break). They touch only `Cell`s and post
//! messages to the Window, which acts after they return.
//!
//! Every function here runs on the UI thread, and none of them holds the
//! state's borrow across a call into WebView2.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationCompletedEventHandler, NavigationStartingEventHandler, NewWindowRequestedEventHandler,
};
use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, ACCEL, FALT, FCONTROL, FSHIFT, FVIRTKEY, WM_COMMAND};

use super::{Page, HOST};
use crate::proto::{WM_RENDERED_FAILED, WM_RENDERED_LINK, WM_RENDERED_SHOWN};

/// `WM_RENDERED_FAILED`'s `WPARAM`: WebView2 couldn't start (no runtime?).
pub const FAILED_TO_START: usize = 0;
/// `WM_RENDERED_FAILED`'s `WPARAM`: `NavigateToString` refused the page.
pub const FAILED_TOO_LARGE: usize = 1;

/// What the Window asked for, and what WebView2 has.
#[derive(Default)]
struct Host {
    hwnd: HWND,
    /// The Window's accelerators, for keys pressed inside the page.
    keys: Rc<Vec<ACCEL>>,
    /// The Esc command: back to the Source view.
    esc: u16,
    /// Bumped by `close`: completions of an older generation are stale.
    generation: u64,
    phase: Phase,
    bounds: RECT,
    /// The page to show; `None` while the active Tab is in the Source view.
    wanted: Option<Rc<Page>>,
    minimized: bool,
}

#[derive(Default)]
enum Phase {
    #[default]
    Off,
    Creating,
    Ready(Live),
}

/// A working WebView2 and what it shows.
#[derive(Clone)]
struct Live {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    nav: Rc<Nav>,
    /// The folder mapped to `HOST`.
    root: Option<PathBuf>,
    /// The page last handed to `NavigateToString`.
    loaded: Option<u64>,
    visible: bool,
}

/// Shared with the navigation handlers.
#[derive(Default)]
struct Nav {
    /// Counted up just before each of scrivi's own `NavigateToString`s: that
    /// many `data:` navigations are allowed. A count, not a flag, so a render
    /// asked for before the last one started isn't cancelled as a stranger.
    /// (The page itself can't link to `data:`: ammonia drops the scheme.)
    render_pending: Cell<u32>,
    /// The id of the latest of those navigations to start.
    ours: Cell<Option<u64>>,
    /// That navigation completed: the page is on screen.
    done: Cell<bool>,
}

thread_local! {
    static HOST_STATE: RefCell<Host> = RefCell::default();
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> R {
    HOST_STATE.with(|h| f(&mut h.borrow_mut()))
}

fn page_key(page: &Page) -> u64 {
    let mut h = DefaultHasher::new();
    page.hash(&mut h);
    h.finish()
}

/// Show `page` over `bounds` (the editor band, in client pixels), creating
/// WebView2 first if needed. `keys` is the Window's accelerator table, so its
/// shortcuts work while the page has the focus; Esc posts `esc`.
pub fn show(hwnd: HWND, keys: &[ACCEL], esc: u16, page: Page, bounds: RECT) {
    set_bounds(bounds);
    let start = with(|h| {
        h.hwnd = hwnd;
        if h.keys.as_slice() != keys {
            h.keys = Rc::new(keys.to_vec());
        }
        h.esc = esc;
        h.wanted = Some(Rc::new(page));
        if matches!(h.phase, Phase::Off) {
            h.phase = Phase::Creating;
            Some(h.generation)
        } else {
            None
        }
    });
    match start {
        Some(generation) => create(generation),
        None => apply(),
    }
}

/// The active Tab is in the Source view, another still in the Rendered view:
/// keep WebView2, out of sight.
pub fn hide() {
    // Called after every command: only a change reaches WebView2.
    if with(|h| h.wanted.take().is_some()) {
        apply();
    }
}

/// Its page finished loading (`WM_RENDERED_SHOWN`): show it, if still wanted.
pub fn reveal() {
    apply();
}

/// No Tab is in the Rendered view, or the Window is going: close WebView2.
/// Its processes exit about 0.2 s later.
pub fn close() {
    let live = with(|h| {
        h.generation += 1;
        h.wanted = None;
        match std::mem::take(&mut h.phase) {
            Phase::Ready(live) => Some(live),
            _ => None,
        }
    });
    if let Some(live) = live {
        unsafe {
            let _ = live.controller.Close();
        }
    }
}

/// The editor band moved or resized.
pub fn set_bounds(bounds: RECT) {
    let live = with(|h| {
        if std::mem::replace(&mut h.bounds, bounds) == bounds {
            return None;
        }
        ready(h)
    });
    if let Some(live) = live {
        unsafe {
            let _ = live.controller.SetBounds(bounds);
        }
    }
}

/// The Window was minimised or restored: a hidden WebView2 gets no window
/// messages, so it must be told (`IsVisible`).
pub fn set_minimized(minimized: bool) {
    let changed = with(|h| std::mem::replace(&mut h.minimized, minimized) != minimized);
    if changed {
        apply();
    }
}

/// The Window moved: WebView2 keeps its popups (IME, ...) in place.
pub fn parent_moved() {
    if let Some(live) = with(ready) {
        unsafe {
            let _ = live.controller.NotifyParentWindowPositionChanged();
        }
    }
}

/// The page is on screen, covering the editor band.
pub fn painted() -> bool {
    with(|h| ready(h).is_some_and(|l| l.visible))
}

/// Give the page the keyboard focus, if it is on screen.
pub fn focus() -> bool {
    let Some(live) = with(ready).filter(|l| l.visible) else { return false };
    unsafe { live.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC).is_ok() }
}

fn ready(h: &mut Host) -> Option<Live> {
    match &h.phase {
        Phase::Ready(live) => Some(live.clone()),
        _ => None,
    }
}

/// Bring WebView2 in line with what the Window wants: the right page, sized
/// to the band, visible only once that page has loaded.
fn apply() {
    let Some((live, wanted, minimized, hwnd)) =
        with(|h| ready(h).map(|l| (l, h.wanted.clone(), h.minimized, h.hwnd)))
    else {
        return;
    };
    let Some(page) = wanted.filter(|_| !minimized) else {
        return set_visible(&live, false);
    };
    let key = page_key(&page);
    if live.loaded != Some(key) {
        // The old page stays up until this one has loaded.
        return navigate(&live, &page, key, hwnd);
    }
    // Still loading: NavigationCompleted posts WM_RENDERED_SHOWN, which comes back here.
    if live.nav.done.get() {
        set_visible(&live, true);
    }
}

fn set_visible(live: &Live, visible: bool) {
    if live.visible == visible {
        return;
    }
    unsafe {
        let _ = live.controller.SetIsVisible(visible);
    }
    with(|h| {
        if let Phase::Ready(l) = &mut h.phase {
            l.visible = visible;
        }
    });
}

fn navigate(live: &Live, page: &Page, key: u64, hwnd: HWND) {
    unsafe {
        if live.root.as_deref() != Some(page.root.as_path()) {
            if let Ok(wv3) = live.webview.cast::<ICoreWebView2_3>() {
                let _ = wv3.ClearVirtualHostNameToFolderMapping(&HSTRING::from(HOST));
                let _ = wv3.SetVirtualHostNameToFolderMapping(
                    &HSTRING::from(HOST),
                    &HSTRING::from(page.root.as_os_str()),
                    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
                );
            }
        }
        // Out of sight until the new page has loaded: the editor shows this
        // Tab's source meanwhile, never another Tab's page.
        set_visible(live, false);
        let nav = &live.nav;
        nav.render_pending.set(nav.render_pending.get() + 1);
        nav.done.set(false);
        let html = HSTRING::from(page.html.as_str());
        let navigated = live.webview.NavigateToString(&html).is_ok();
        if !navigated {
            nav.render_pending.set(nav.render_pending.get().saturating_sub(1));
        }
        let root = page.root.clone();
        with(|h| {
            if let Phase::Ready(l) = &mut h.phase {
                l.root = Some(root);
                l.loaded = navigated.then_some(key);
            }
        });
        if !navigated {
            post(hwnd, WM_RENDERED_FAILED, FAILED_TOO_LARGE, 0);
        }
    }
}

fn post(hwnd: HWND, msg: u32, wp: usize, lp: isize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), msg, WPARAM(wp), LPARAM(lp));
    }
}

fn fail(generation: u64) {
    let hwnd = with(|h| {
        let current = h.generation == generation;
        if current {
            h.phase = Phase::Off;
            h.wanted = None;
        }
        current.then_some(h.hwnd)
    });
    if let Some(hwnd) = hwnd {
        post(hwnd, WM_RENDERED_FAILED, FAILED_TO_START, 0);
    }
}

fn is_current(generation: u64) -> bool {
    with(|h| h.generation == generation && matches!(h.phase, Phase::Creating))
}

/// `%LOCALAPPDATA%\scrivi\WebView2`: never next to the exe, which may not be writable.
fn user_data_folder() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("scrivi").join("WebView2"))
}

fn create(generation: u64) {
    unsafe {
        // WebView2 needs an STA; already one if anything else set up COM.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let options = CoreWebView2EnvironmentOptions::default();
        // Edge's mini menu and SmartScreen have nothing to do on a local page.
        options.set_additional_browser_arguments("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".into());
        let options: ICoreWebView2EnvironmentOptions = options.into();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env| {
            on_environment(generation, result, env);
            Ok(())
        }));
        let folder = user_data_folder().map(|f| HSTRING::from(f.as_os_str())).unwrap_or_default();
        let folder = if folder.is_empty() { PCWSTR::null() } else { PCWSTR(folder.as_ptr()) };
        if CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), folder, &options, &handler).is_err() {
            fail(generation);
        }
    }
}

fn on_environment(generation: u64, result: windows::core::Result<()>, env: Option<ICoreWebView2Environment>) {
    if !is_current(generation) {
        return;
    }
    let Some(env) = env.filter(|_| result.is_ok()) else { return fail(generation) };
    let hwnd = with(|h| h.hwnd);
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
        on_controller(generation, result, controller);
        Ok(())
    }));
    if unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) }.is_err() {
        fail(generation);
    }
}

fn on_controller(generation: u64, result: windows::core::Result<()>, controller: Option<ICoreWebView2Controller>) {
    if !is_current(generation) {
        // Closed while it was being made: close it now, unshown.
        if let Some(controller) = controller {
            unsafe {
                let _ = controller.Close();
            }
        }
        return;
    }
    let Some(controller) = controller.filter(|_| result.is_ok()) else { return fail(generation) };
    let (hwnd, keys, esc, bounds) = with(|h| (h.hwnd, Rc::clone(&h.keys), h.esc, h.bounds));
    let Ok(live) = (unsafe { configure(controller.clone(), hwnd, keys, esc, bounds) }) else {
        unsafe {
            let _ = controller.Close();
        }
        return fail(generation);
    };
    with(|h| h.phase = Phase::Ready(live));
    apply();
}

/// Settings for a static, read-only page, and the handlers.
unsafe fn configure(
    controller: ICoreWebView2Controller,
    hwnd: HWND,
    keys: Rc<Vec<ACCEL>>,
    esc: u16,
    bounds: RECT,
) -> windows::core::Result<Live> {
    unsafe {
        controller.SetIsVisible(false)?;
        controller.SetBounds(bounds)?;
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: 255, G: 255, B: 255 });
        }
        // A file dropped on the page must not navigate to it.
        if let Ok(c4) = controller.cast::<ICoreWebView2Controller4>() {
            let _ = c4.SetAllowExternalDrop(false);
        }
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;
        settings.SetIsScriptEnabled(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
            // Find, print, reload, zoom keys... Copy and select all stay.
            let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
        }
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            let _ = s4.SetIsGeneralAutofillEnabled(false);
            let _ = s4.SetIsPasswordAutosaveEnabled(false);
        }

        let nav = Rc::new(Nav::default());
        let mut token = 0i64;
        {
            let nav = Rc::clone(&nav);
            let handler = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if nav.render_pending.get() > 0 && uri.starts_with("data:") {
                    // scrivi's own render; a later one supersedes an earlier.
                    nav.render_pending.set(nav.render_pending.get() - 1);
                    let mut id = 0u64;
                    args.NavigationId(&mut id)?;
                    nav.ours.set(Some(id));
                    nav.done.set(false);
                    return Ok(());
                }
                args.SetCancel(true)?;
                open_outside(hwnd, &uri);
                Ok(())
            }));
            webview.add_NavigationStarting(&handler, &mut token)?;
        }
        {
            let nav = Rc::clone(&nav);
            let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut id = 0u64;
                args.NavigationId(&mut id)?;
                let mut success = windows::core::BOOL::default();
                args.IsSuccess(&mut success)?;
                if nav.ours.get() == Some(id) && success.as_bool() {
                    nav.done.set(true);
                    // Shown from the Window's message loop, not from inside WebView2.
                    post(hwnd, WM_RENDERED_SHOWN, 0, 0);
                }
                Ok(())
            }));
            webview.add_NavigationCompleted(&handler, &mut token)?;
        }
        {
            // target=_blank, Ctrl+click, middle-click: open outside too.
            let handler = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                args.SetHandled(true)?;
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                open_outside(hwnd, &take_pwstr(uri));
                Ok(())
            }));
            webview.add_NewWindowRequested(&handler, &mut token)?;
        }
        {
            let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                args.KeyEventKind(&mut kind)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                    return Ok(());
                }
                let mut vk = 0u32;
                args.VirtualKey(&mut vk)?;
                let down = |k: u16| GetKeyState(k as i32) < 0;
                let pressed = Pressed { vk, ctrl: down(VK_CONTROL.0), shift: down(VK_SHIFT.0), alt: down(VK_MENU.0) };
                let Some(command) = command_for(&keys, esc, pressed) else { return Ok(()) };
                args.SetHandled(true)?;
                let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                args.PhysicalKeyStatus(&mut status)?;
                // A held key repeats: act on the first press only.
                if !status.WasKeyDown.as_bool() {
                    post(hwnd, WM_COMMAND, command as usize, 0);
                }
                Ok(())
            }));
            controller.add_AcceleratorKeyPressed(&handler, &mut token)?;
        }
        Ok(Live { controller, webview, nav, root: None, loaded: None, visible: false })
    }
}

/// A key pressed while the page has the focus.
#[derive(Clone, Copy, Debug)]
pub struct Pressed {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The Window command a key runs: Esc goes back to the Source view, the rest
/// as the Window's accelerator table says. `None` leaves the key to the page
/// (arrows, Ctrl+C, ...).
pub fn command_for(keys: &[ACCEL], esc: u16, p: Pressed) -> Option<u16> {
    if p.vk == VK_ESCAPE.0 as u32 && !(p.ctrl || p.shift || p.alt) {
        return Some(esc);
    }
    keys.iter()
        .find(|a| {
            let f = a.fVirt;
            f.contains(FVIRTKEY)
                && a.key as u32 == p.vk
                && f.contains(FCONTROL) == p.ctrl
                && f.contains(FSHIFT) == p.shift
                && f.contains(FALT) == p.alt
        })
        .map(|a| a.cmd)
}

/// http, https and mailto links go to the default handler; anything else a
/// link points at (another file, `about:`, ...) goes nowhere. The Window opens
/// it once this handler has returned.
fn open_outside(hwnd: HWND, uri: &str) {
    if super::opens_outside(uri) {
        let raw = Box::into_raw(Box::new(uri.to_string()));
        unsafe {
            if PostMessageW(Some(hwnd), WM_RENDERED_LINK, WPARAM(0), LPARAM(raw as isize)).is_err() {
                // Never delivered: nothing else will free it.
                drop(Box::from_raw(raw));
            }
        }
    }
}

/// The user's Documents folder, where an Untitled tab's images resolve.
pub fn documents_folder() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// The folder a Tab's relative images resolve against: its file's folder, or
/// Documents for an Untitled tab (SCRIVI-21, 2026-10-26).
pub fn folder_for(path: Option<&Path>) -> PathBuf {
    match path.and_then(Path::parent) {
        Some(dir) => dir.to_path_buf(),
        None => documents_folder().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEXT: u16 = 102;
    const PREV: u16 = 103;
    const TOGGLE: u16 = 140;
    const ESC: u16 = 145;

    fn keys() -> Vec<ACCEL> {
        let ctrl = FCONTROL | FVIRTKEY;
        vec![
            ACCEL { fVirt: ctrl, key: 0x09, cmd: NEXT },
            ACCEL { fVirt: ctrl | FSHIFT, key: 0x09, cmd: PREV },
            ACCEL { fVirt: ctrl, key: b'E' as u16, cmd: TOGGLE },
        ]
    }

    fn press(vk: u32, ctrl: bool, shift: bool) -> Option<u16> {
        command_for(&keys(), ESC, Pressed { vk, ctrl, shift, alt: false })
    }

    #[test]
    fn keys_in_the_page_run_the_windows_shortcuts() {
        assert_eq!(press(b'E' as u32, true, false), Some(TOGGLE));
        assert_eq!(press(0x09, true, false), Some(NEXT));
        assert_eq!(press(0x09, true, true), Some(PREV));
        assert_eq!(press(VK_ESCAPE.0 as u32, false, false), Some(ESC));
    }

    #[test]
    fn other_keys_stay_with_the_page() {
        // Ctrl+C copies, arrows scroll, a plain E or Shift+Esc does nothing of ours.
        assert_eq!(press(b'C' as u32, true, false), None);
        assert_eq!(press(0x28, false, false), None);
        assert_eq!(press(b'E' as u32, false, false), None);
        assert_eq!(press(b'E' as u32, true, true), None);
        assert_eq!(press(VK_ESCAPE.0 as u32, false, true), None);
    }
}
