# scrivi: the Rendered view in WebView2, on demand

Research date 2026-10-26, for Traccia SCRIVI-5. Research only: no app code. It builds on `research/borrowable-code.md` (branch `research/borrowable-code`), whose "Hybrid" and §4 Weight sections already cover the basics of `webview2-com`, `NavigateToString` and WebView2 weight. Terms follow [GLOSSARY.md](../GLOSSARY.md).

Sources are MS Learn's WebView2 reference and concept pages, the `webview2-com` 0.39.1 and `wry` 0.57.0 sources, and the licence files in their repos. Behaviour that the docs leave open, or get wrong, was measured with a throwaway Rust prototype (`webview2-com` 0.39.1 + `windows` 0.62.2, raw Win32, release build). The prototype was built and run outside the repo; see [Method](#method).

**Contents**
- [Answer in brief](#answer-in-brief)
- [Surprises that affect the design](#surprises-that-affect-the-design)
- [1. Hosting](#1-hosting)
- [2. Content](#2-content)
- [3. Local images](#3-local-images)
- [4. Links, context menu, devtools, new windows](#4-links-context-menu-devtools-new-windows)
- [5. Cost (measured)](#5-cost-measured)
- [6. Prior art](#6-prior-art)
- [Still unverified](#still-unverified)
- [Method](#method)

## Answer in brief

- **Hosting.** Use `webview2-com` **0.39.1** (MIT, built on `windows-core` 0.62, matching scrivi's `windows` 0.62.2). On MSVC it links `WebView2LoaderStatic.lib`, so there is no loader DLL to ship. The release prototype exe is 369 KB.
  - Create the environment with a user data folder under `%LOCALAPPDATA%\scrivi\WebView2`. The default (next to the exe) fails under Program Files.
  - Create the controller with scrivi's main window as parent, and set `Bounds` to the tab content rect.
  - Drive creation from async completion handlers on the UI thread. Don't use a nested pump.
  - Resize on `WM_SIZE`, call `NotifyParentWindowPositionChanged` on `WM_MOVE`, and set `IsVisible` false when minimised.
  - Keyboard: Esc is "always considered an accelerator" and Ctrl+E is one too. Both reach `AcceleratorKeyPressed`. Mark them `Handled` and `PostMessage` the command to the window.
  - Teardown: `Close()` the controller, then release it. The browser process tree had exited **~160–230 ms** after `Close()` in every run.
- **Content.** Use pulldown-cmark (already the Source view's parser) plus ammonia. Add one `<style>` after sanitising, set `IsScriptEnabled = false`, then call `NavigateToString`. The real limit is **~1.5 MiB of UTF-8 HTML**, not 2 MB. Anything bigger needs a fallback.
- **Local images.** Yes, use `SetVirtualHostNameToFolderMapping`, but with three corrections:
  1. Use access kind **`DENY_CORS`**. `DENY` blocks images on a `NavigateToString` page, even though Microsoft's own sample uses `DENY`.
  2. Relative URLs don't resolve, so rewrite image URLs to absolute `https://<host>/...` while rendering. A `<base href>` would fix images but breaks `#anchor` links.
  3. `../` can't climb above the mapped folder, so map the nearest common ancestor of the file and the images it references.
- **Links.** Cancel every `NavigationStarting` except scrivi's own. Note that its own shows up as `data:text/html;charset=utf-8;base64,...`, not `about:blank`. Open http/https/mailto in the default handler. `#fragment` links don't fire `NavigationStarting` and just scroll. `target=_blank` links fire `NewWindowRequested`: set `Handled` and open externally. Settings: default context menus off, devtools off, browser accelerator keys off, external drop off.
- **Cost (this laptop, Edge already running).**

  | | Measured |
  |---|---|
  | Toggle to `NavigationCompleted` | **0.28–0.52 s** (one 1.1 s outlier) |
  | Memory while open | **~92–97 MB** private working set, **~150 MB** private bytes, **6 processes**. scrivi's own process stayed at ~2 MB. |
  | Hidden, with `MemoryUsageTargetLevel` Low | only −12 MB |
  | Processes exit after `Close()` | **~0.2 s** |

  Keeping one environment alive and recreating only the controller is **not worth it**. The processes exit anyway, even with the environment object alive. Recreating on the same environment started a new browser process and took the same time (0.26–0.46 s against 0.25–1.1 s for a fresh environment). Creating an environment costs 1–9 ms.
- **Prior art.** wry 0.57 `src/webview2/mod.rs` (MIT OR Apache-2.0) and `webview2-com`'s `examples/sample.rs` (MIT). Both are borrowable. `microsoft/windows-samples-rs`, which the sample was adapted from, is archived and has no licence file, so read it but don't copy from it.

## Surprises that affect the design

1. **The 2 MB limit is really ~1.5 MiB of UTF-8.** `NavigateToString` turns the HTML into a base64 `data:` URL, and that URL is what is capped. Measured: 1,570,000 ASCII bytes passed and 1,580,000 failed with `E_INVALIDARG`. 520,000 × `€` (1.56 MB UTF-8, 1.04 MB UTF-16) passed and 530,000 failed. The docs say "2 MB (2 * 1024 * 1024 bytes)".
2. **Scrivi's own navigation is a `data:` URL.** `NavigationStarting` reports `data:text/html;charset=utf-8;base64,…`, while `location.href` in the page is `about:blank`. A handler that allows only `about:blank` would cancel scrivi's own render.
3. **`DENY` blocks images on a `NavigateToString` page.** The page's origin is `null`, so every mapped resource counts as cross-origin. `DENY` is defined as denying "normal sub resource access as src of a script or image element". Microsoft's own `NavigateToString` sample on the same reference page uses `DENY` and claims the image shows. Measured: under `DENY` no image loaded; under `DENY_CORS` and `ALLOW` they did.
4. **`<base href>` breaks in-page anchors.** With `<base href="https://doc.example/">`, `#sec` resolves to `https://doc.example/#sec`. That is a cross-document navigation, which scrivi would cancel and open in a browser. Rewrite image URLs instead and leave links relative.
5. **Teardown frees the processes promptly, and keeping the environment doesn't keep them.** `BrowserProcessExited` fired 159–204 ms after `Close()` while the environment object was still referenced, and all child processes were gone 10–17 ms later. Dropping the controller without `Close()` also freed them, in 200–304 ms. The docs still ask for explicit `Close()` to break handler reference cycles.
6. **Large documents leave the renderer bloated until teardown.** After a few 1.5 MB pages, the tree sat at 272 MB private working set. Hiding it with the Low memory target only brought that to 256 MB. Tearing down is the only real reclaim.

## 1. Hosting

### Crate

- **`webview2-com` 0.39.1**, 2026-03-11, the latest on crates.io. MIT, verified from [wravery/webview2-rs LICENSE](https://github.com/wravery/webview2-rs/blob/main/LICENSE) ("MIT License, Copyright (c) 2021 Bill Avery"). It depends on `windows` 0.62 and `windows-core` 0.62, the same line as scrivi's pinned `=0.62.2`. Bindings are for WebView2 SDK 1.0.3650.58 (`webview2-com-sys` CHANGELOG).
- **Linking.** `webview2-com-sys` links `WebView2LoaderStatic` on `target_env = "msvc"` and `WebView2Loader.dll` only on gnu (`src/lib.rs`, `link_webview2!`). On MSVC nothing extra ships.
- **Shape of the API.** It is the raw COM projection, mostly `unsafe`. The crate adds closure-based handler types (`CreateCoreWebView2EnvironmentCompletedHandler`, `NavigationStartingEventHandler`, `AcceleratorKeyPressedEventHandler`, `BrowserProcessExitedEventHandler`, …), `CoreWebView2EnvironmentOptions`, and the PWSTR helpers `take_pwstr` and `string_from_pcwstr` ([README](https://github.com/wravery/webview2-rs/tree/main/crates/webview2-com)).
- **Avoid `wait_with_pump` in the app.** `webview2_com::wait_with_pump` runs a nested `GetMessageA` loop until the async result arrives. wry uses it, and so did the prototype for brevity. Microsoft's threading model says callbacks "run serially", and that a nested message loop inside a WebView2 handler "isn't supported" and "would leave the event handler in the stack indefinitely" ([Threading model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model)). For scrivi, make the Rendered view a small state machine instead: `Off → CreatingEnv → CreatingController → Ready → Closing`. Each completion handler advances it. A toggle that arrives mid-creation just records the wanted state.

### Environment and user data folder

- Call `CreateCoreWebView2EnvironmentWithOptions(null, udf, options, handler)` on the UI thread. The thread must be STA with a message pump: "All callbacks occur on that thread" ([Threading model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model)).
- **User data folder.** The Win32 default is `<exe path>.WebView2` next to the exe. Microsoft: "In most cases, you should specify a custom UDF location". If the app runs from a folder the user can't write to, "you will receive a runtime error during WebView2 startup" ([Manage user data folders](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder)). Use `%LOCALAPPDATA%\scrivi\WebView2`. The folder is not removed automatically, so the uninstaller should delete it.
- **Options.** Copy wry's `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`, which removes Edge's "mini menu" and SmartScreen (wry `create_environment`). SmartScreen is moot here because scrivi never loads remote pages. Leave `Language` at the default, or set it once and never change it. Two environments on the same folder with different options "will fail to create `WebView2` objects" ([Process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)).
- **Runtime missing (old Windows 10).** Environment creation then fails. Show the Source view only and a one-line message. The runtime ships with Windows 11 (see `borrowable-code.md`).

### Controller as a child of scrivi's window

- Call `env.CreateCoreWebView2Controller(hwnd, handler)`. WebView2 creates its own child HWNDs under `hwnd`. `Bounds` "are relative to the parent `HWND`". Microsoft gives two layouts: a dedicated child HWND with bounds at (0,0), or the top window with the bounds offset ([ICoreWebView2Controller::get_Bounds](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)). For scrivi, parent to the main window and set `Bounds` to the tab content rect (below the tab strip). Use `WS_CLIPCHILDREN` on the main window, as wry does.
- **Resizing.** On `WM_SIZE` (not minimised), recompute the tab content rect and call `SetBounds`. On `WM_MOVE`/`WM_MOVING`, call `NotifyParentWindowPositionChanged`. This is wry's `parent_subclass_proc`, which keeps WebView2 popups and IME windows in place.
- **Minimise.** Microsoft: "developers should set the `IsVisible` property of the WebView to `FALSE` when the app window is minimized and back to `TRUE` when the app window is restored". A child WebView2 "does not get window messages when the top window is minimized" ([get_IsVisible](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)).
- **Switching to a tab in the Source view** while another tab still has the Rendered view: set `IsVisible(false)` and keep the controller. That fits the decided rule (WebView2 lives while any tab is in the Rendered view).
- **One controller for all tabs.** On a tab switch, `NavigateToString` the other tab's HTML (a 17 KB page took ~70–180 ms in the measurements). `ExecuteScript` runs "even if `IsScriptEnabled` is set to `FALSE`" ([ICoreWebView2::ExecuteScript](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2)), and the prototype confirmed it. So the host can save `scrollY` before re-rendering (tab switch, edit, disk change) and restore it after `NavigationCompleted`.

### Focus and keyboard (Esc and Ctrl+E must reach scrivi)

- When the tab enters the Rendered view, call `controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)`. wry does the same on the parent's `WM_SETFOCUS`.
- While WebView2 has focus, scrivi's window gets no `WM_KEYDOWN`. Keys go to `AcceleratorKeyPressed` instead, which fires when "Ctrl or Alt is currently being held" or "the pressed key does not map to a character", and "The `Escape` key is always considered an accelerator" ([add_AcceleratorKeyPressed](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)). So Esc, Ctrl+E, Ctrl+Tab, Ctrl+W, Ctrl+S and F-keys all reach the handler.
- **The handler runs synchronously** in windowed mode: "the browser process is blocked" until it returns. So look the key up in scrivi's own accelerator table. On a match, call `Handled(TRUE)` and `PostMessage(hwnd, WM_COMMAND, id)`. Ignore autorepeat via `PhysicalKeyStatus.WasKeyDown`. This is Microsoft's own sample pattern.
- **`AreBrowserAcceleratorKeysEnabled = FALSE`** (Settings3) disables Ctrl+F/F3, Ctrl+P, Ctrl+R/F5, Ctrl+±, Ctrl+Shift+C/F12 and the browser keys. It keeps Ctrl+C, Ctrl+A, Home/End and PgUp/PgDn. It "has no effect on the `AcceleratorKeyPressed` event" ([ICoreWebView2Settings3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2settings3)). Settings apply "at the next navigation", so set them before the first `NavigateToString`.
- *Not exercised by the prototype.* Injecting real keystrokes (`SendInput`) could type into whatever window the user has focused, so this part rests on the docs above.

### Teardown

- `controller.Close()`, then drop the controller, `ICoreWebView2` and every handler. Drop the environment too, since keeping it buys nothing (see Cost).
- `Close` "Closes the WebView and cleans up the underlying browser instance". It "is implicitly run when the `CoreWebView2Controller` loses the final reference". Microsoft still says "it is best practice to explicitly run `Close`" to break handler reference cycles ([Close](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)). In Rust that means handler closures must not capture the controller or webview. Pass state through `Rc<RefCell<…>>` or window messages instead.
- `BrowserProcessExited` (`ICoreWebView2Environment5`) is raised when the runtime processes "terminate due to browser process failure or normal shutdown (for example, when all associated WebViews are closed)" ([Environment5](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment5)). scrivi only needs it if it ever deletes the user data folder. Measured timings are in [Cost](#5-cost-measured).
- On `WM_DESTROY` of the main window, `Close()` first. wry's `Drop` does `controller.Close()` then `DestroyWindow` of its container.

## 2. Content

- **Parser.** pulldown-cmark already drives the Source view's styling (`borrowable-code.md` §2), so render HTML from the same event stream with `pulldown_cmark::html::push_html`. Its HTML writer passes raw HTML through "verbatim", so sanitise with **ammonia** 4.x (MIT OR Apache-2.0). comrak with its default safe mode is the alternative when full GFM autolinks are wanted. Licences and versions are in `borrowable-code.md`.
- **Page shape.** `<!doctype html><meta charset=utf-8><style>…one embedded stylesheet…</style>` plus the sanitised body. Add the `<style>` after sanitising, because ammonia strips `<style>`.
- **Settings for a static page.**
  - `IsScriptEnabled = false`. Microsoft recommends this "when showing static HTML content" ([Security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security)).
  - `IsWebMessageEnabled = false` and `AreHostObjectsAllowed = false`.
  - `IsStatusBarEnabled = false`.
  - `IsGeneralAutofillEnabled = false` and `IsPasswordAutosaveEnabled = false`.

  All were set in the prototype without error.
- **Size limit, measured.** `NavigateToString` fails synchronously with `0x80070057` (E_INVALIDARG; the docs' "Value does not fall within the expected range") when the base64 `data:` URL passes 2 MiB. That puts the practical cap at **~1,572,000 bytes of UTF-8 HTML** (1,570,000 passed and 1,580,000 failed). Above that, the options are:
  - show a "Too large for the Rendered view" notice (simplest; a 1.5 MB HTML page means a markdown file well past 1 MB);
  - serve the page from a virtual URL through `WebResourceRequested` (`NavigateWithWebResourceRequest` or `Navigate` plus a filter). That has no size cap but is "slower … due to the cross-process communication" ([Using local content](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/working-with-local-content));
  - write the HTML to `%LOCALAPPDATA%\scrivi\render\` and map that folder to a virtual host. Never write into the user's own folder.
- **Rendering cost** of pulldown-cmark plus ammonia was not measured. Both are linear-time, and next to the 250–500 ms WebView2 start it is noise for normal files.

## 3. Local images

**Is `SetVirtualHostNameToFolderMapping` the answer? Yes, with the corrections below.**

Facts from the docs:
- `NavigateToString` content "doesn't make it possible for the resulting document to reference additional web resources", and its origin is `null` with location `about:blank` ([Using local content](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/working-with-local-content)).
- The mapping "applies to both top-level document and iframe navigations as well as subresource references". Microsoft's own example embeds `<img src="http://appassets.example/wv2.png"/>` in a `NavigateToString` page.
- "changes to the mapping might not be applied to the current page and a reload of the page is needed". `folderPath` must not exceed `MAX_PATH`. Use reserved TLDs (`.example`, `.test`, `.invalid`) and avoid `.local`, which "can cause a delay during navigations". "Apps should use distinct domain names when mapping folder from different sources that should be isolated from each other" ([ICoreWebView2_3::SetVirtualHostNameToFolderMapping](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3)).
- **Access kinds** ([CoreWebView2HostResourceAccessKind](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2hostresourceaccesskind)):
  - `Deny`: "All cross origin resource access is denied, including normal sub resource access as src of a script or image element."
  - `Allow`: everything, like `Access-Control-Allow-Origin: *`.
  - `DenyCors`: img/script/iframe `src` allowed, fetch/XHR denied.

Measured on a `NavigateToString` page, with the mapping `doc.example → <docs>\notes`:

| Image reference in the HTML | DENY | DENY_CORS | ALLOW |
|---|---|---|---|
| `https://doc.example/img.svg` (absolute) | no | **loaded** | loaded |
| `http://doc.example/img.svg` | no | loaded | loaded |
| `https://doc.example/img.png` | no | loaded | loaded |
| `img.svg` (relative, no `<base>`) | no | no | no |
| `img.svg` with `<base href="https://doc.example/">` | no | loaded | loaded |
| `https://doc.example/../outside.svg` | no | no | no |
| `file:///…/notes/img.svg` | no | no | no |

Recommendation:
1. **Map with `COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS`.** With scripts off, fetch/XHR can't happen anyway, but DENY_CORS is the minimal kind that lets `<img>` load.
2. **Rewrite image URLs while rendering.** In the pulldown-cmark event stream, map each relative `Tag::Image { dest_url }` (and raw `<img src>`, via ammonia's `UrlRelative::Custom` evaluator) to `https://scrivi-doc.example/<percent-encoded path>`. Leave relative *links* alone so `#anchors` stay same-document. Don't use `<base href>` (Surprise 4).
3. **Root of the mapping.** The URL is normalised before it reaches the mapping, so `../` cannot climb above the mapped folder (measured). Since scrivi knows every image path at render time, map the **nearest common ancestor** of the file's folder and all resolved image paths, and write URLs relative to that root. Add a sanity bound: if the ancestor would be a drive root or the user profile, map only the file's folder and let outside images fail. Re-map on every render. scrivi navigates afresh each time anyway, so the "reload needed" caveat doesn't bite.
4. **Exposure.** The page can load anything under the mapped root by URL, but only through `<img>`/`<link>` tags in HTML that scrivi itself generated and sanitised, with scripts off. That is the same trust as opening the markdown in any viewer. Absolute `file://` images don't load from a `null`-origin page (measured), and ammonia's default URL schemes don't include `file`.

## 4. Links, context menu, devtools, new windows

Measured, with scripts off and the clicks injected by `ExecuteScript`:

| Action | Event | What scrivi does |
|---|---|---|
| Scrivi's own `NavigateToString` | `NavigationStarting`, uri `data:text/html;charset=utf-8;base64,…` | Allow. Set a "render pending" flag just before calling it, and allow exactly the next navigation. |
| `<a href="https://…">` click | `NavigationStarting`, uri `https://…` | `SetCancel(TRUE)`, then `ShellExecuteW("open", uri)` after the handler returns |
| `<a href="mailto:…">` click | `NavigationStarting`, uri `mailto:…` | Cancel, then open (whitelist `http`, `https`, `mailto`) |
| `<a href="#sec">` click | **none**. It scrolls in place; `location` became `about:blank#sec` | nothing |
| `<a target=_blank>` (also Ctrl/middle-click) | `NewWindowRequested` | `SetHandled(TRUE)`, then open externally. "If either `Handled` or `NewWindow` properties are not set, the target content will be opened on a popup window" ([add_NewWindowRequested](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2)). |
| Relative link to another file (`other.md`) | `NavigationStarting` | Cancel. An option is to open it in scrivi: resolve against the file's folder and open it as a new tab. |

- "Navigations will be blocked until all `NavigationStarting` event handlers return", so do the `ShellExecuteW` via `PostMessage`, not inside the handler (the threading model's reentrancy rule).
- **Context menu.** `SetAreDefaultContextMenusEnabled(FALSE)` removes the whole menu, Copy included. Ctrl+C still works, because Settings3 keeps editing keys. If a right-click "Copy" is wanted, filter the menu in `ContextMenuRequested` (`ICoreWebView2_11`) instead. That was not tested here.
- **Devtools.** `SetAreDevToolsEnabled(FALSE)`. F12 and Ctrl+Shift+C are also covered by browser accelerator keys being off.
- **External drop.** `ICoreWebView2Controller4::SetAllowExternalDrop(FALSE)` (wry does the same). Without it, dropping a file onto the Rendered view navigates the WebView to that file instead of letting scrivi open it. scrivi's own drop target on the main window then handles it.

## 5. Cost (measured)

Same machine as `borrowable-code.md` §4: i7-1370P, 64 GB, Windows 11 Pro 25H2 build 26200, WebView2 Runtime 155.0.4283.62 (Evergreen). **Edge (14 processes) and other WebView2 apps were running**, so the runtime binaries were warm in memory. Microsoft notes startup is faster in that case ([Performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)). A first toggle after boot without Edge running will be slower, and that was not measured.

The page was 17 KB of HTML (60 sections: headings, lists, code, links) with one stylesheet. Memory is the private working set and private bytes summed over scrivi's process and all its descendants (`Win32_PerfRawData_PerfProc_Process`), the same method as §4.

**Time from "show Rendered view" to `NavigationCompleted`.** The main window was already up. Times are cumulative from the start of the toggle.

| Run | Environment | + controller | NavigationCompleted |
|---|---|---|---|
| 1 (fresh user data folder, first ever) | 8 ms | 335 ms | **517 ms** |
| 2 (`limit` mode) | 6 ms | 254 ms | 382 ms |
| 3 | 6 ms | 205 ms | 278 ms |
| 4 | 9 ms | 338 ms | 521 ms |
| 5 | 8 ms | 287 ms | 467 ms |

**Re-creation within the same process, after a full teardown.**

| | Run 3 | Run 4 | Run 5 |
|---|---|---|---|
| Same environment object kept, new controller | 263 ms | 255 ms | 456 ms |
| Fresh environment + controller | 487 ms | 250 ms | 1,106 ms |

**Memory while open** (visible, idle).

| | Private WS (tree) | Private bytes (tree) | Processes | scrivi host process |
|---|---|---|---|---|
| 4 s after load | 92–97 MB | 150–155 MB | 1 + 6 `msedgewebview2.exe` | 1.9–2.2 MB |
| 10 s after load | 92–97 MB | 151–155 MB | 7 | ~2 MB |
| Hidden (`IsVisible` false), 5 s | 92–97 MB (no change) | 151–155 MB | 7 | |
| Hidden + `MemoryUsageTargetLevel` Low, 5 s | 80–84 MB | 150–155 MB | 7 | |
| After 1.5 MB pages, then back to the 17 KB page | **272 MB** | 337 MB | 7 | |
| …then hidden + Low | 256 MB | 332 MB | 7 | |

**Teardown.**

| | Run 1 | Run 3 | Run 4 | Run 5 |
|---|---|---|---|---|
| `Close()` call | 18 ms | 10 ms | 16 ms | 12 ms |
| `Close()` → `BrowserProcessExited` | 204 ms | 159 ms | 191 ms | 193 ms |
| → all child processes gone | +11 ms | +11 ms | +17 ms | +10 ms |
| Drop without `Close()` → children gone | 252 ms | 200 ms | 213 ms | 304 ms |

**Is it worth keeping one environment alive and recreating only the controller?** No.
- The environment object is a handle on the user data folder. Creating it costs 1–9 ms.
- Closing the last controller shuts the browser process down even while the environment is referenced. `BrowserProcessExited` fired with the environment alive, and the next controller got a new browser PID.
- Recreating on the kept environment (255–456 ms) was not faster than a fresh environment (250–1,106 ms). Both are dominated by the browser process start.

The only way to skip that start is to keep a **controller** alive, hidden, at ~80–97 MB private working set and ~150 MB committed. That contradicts the decided "destroyed when none is" rule, and the measured saving is ~0.3–0.5 s. A short grace period before teardown (for example 30 s after the last tab leaves the Rendered view) would absorb quick Ctrl+E toggling at that cost. This is a tuning option, not a recommendation to re-decide.

## 6. Prior art

- **wry 0.57.x, `src/webview2/mod.rs` and `drag_drop.rs`**: [tauri-apps/wry](https://github.com/tauri-apps/wry). Licence MIT OR Apache-2.0, verified from [LICENSE-MIT](https://github.com/tauri-apps/wry/blob/dev/LICENSE-MIT) ("Copyright (c) 2020-2023 Ngo Iok Ui & Tauri Programme within The Commons Conservancy"), [LICENSE-APACHE](https://github.com/tauri-apps/wry/blob/dev/LICENSE-APACHE) and `LICENSE.spdx` (`PackageLicenseDeclared: Apache-2.0`, `MIT`). The latest release is wry-v0.57.1, 2026-10-23. It uses `webview2-com` directly, with no Tauri. Worth borrowing:
  - `create_environment` / `create_controller`: options, `ICoreWebView2Environment10` controller options (background colour, private mode).
  - `set_webview_settings`: the full settings list.
  - `attach_handlers`: the `NavigationStarting` cancel pattern and `NewWindowRequested`.
  - `parent_subclass_proc`: `WM_SIZE` → `SetBounds`, `WM_SETFOCUS` → `MoveFocus`, `WM_MOVE` → `NotifyParentWindowPositionChanged`, and cleanup on `WM_DESTROY`.
  - `Drop for InnerWebView`: `Close()` and then `DestroyWindow`.
  - `drag_drop.rs`: its own `IDropTarget` after `SetAllowExternalDrop(false)`.

  Not worth borrowing:
  - It blocks with `wait_with_pump` during creation.
  - It has no `AcceleratorKeyPressed` handling, because it relies on the host (tao) to route keys.
- **webview2-com `examples/sample.rs`** (MIT, same repo as the crate): a complete raw Win32 window plus WebView2 in ~670 lines. It covers `CoInitializeEx(COINIT_APARTMENTTHREADED)`, environment and controller creation, `SetBounds` on `WM_SIZE`, and `Close()` in `Drop`. It uses `wait_for_async_operation` (a nested pump) as well.
- **microsoft/windows-samples-rs `webview2_win32`**: the origin of `sample.rs`. Archived (last push 2022-01-25) and the GitHub API reports **no licence**. Read it, don't copy from it.
- **Microsoft WebView2APISample (C++)**: the reference-page snippets quoted above (`CloseWebView`, accelerator handling, `SetVirtualHostNameToFolderMapping`) come from it. The patterns are fine to follow. Copying code from it needs that repo's own licence checked first, which was not done here.

## Still unverified

- **True cold start** (first toggle after boot, Edge not running). Expect it to be noticeably slower than 0.3–0.5 s.
- **Real keystrokes.** Esc, Ctrl+E and Ctrl+Tab through `AcceleratorKeyPressed` were not injected (see §1), and IME and AltGr behaviour wasn't checked either.
- **`ContextMenuRequested`** filtering down to Copy / Select all.
- **Per-monitor DPI** moves (`RasterizationScale`, `ShouldDetectMonitorScaleChanges`) and zoom-with-Ctrl+wheel with browser keys off.
- **Rendering time** of pulldown-cmark plus ammonia on large files, and the exe-size delta of `webview2-com` against a bare window. The prototype exe is 369 KB in total.
- **Memory with real documents** containing many images.

## Method

- A throwaway Cargo project outside the repo: `webview2-com =0.39.1`, `windows =0.62.2`, release profile with `lto`, `codegen-units = 1`, `panic = "abort"` and `strip`. Built offline from the local registry cache.
- The program:
  1. Shows a 1100×800 raw Win32 window.
  2. Creates environment and controller on demand, applies scrivi's settings and handlers, and `NavigateToString`s the 17 KB page.
  3. Measures memory with PowerShell over the process tree (found through a Toolhelp snapshot).
  4. Runs the size-limit, image and link tests. Image results come from `ExecuteScript` reading `img.complete && naturalWidth > 0`; links were clicked with `ExecuteScript` `element.click()`.
  5. Tears down and times `BrowserProcessExited` and the disappearance of child processes, then repeats creation on the same environment and on a fresh one.
- Modes: `all` (everything, run 1), `limit` (fine 2 MB boundary, run 2) and `basic` (clean memory and timing, runs 3–5).
- Timings come from `Instant` around the async operations, with completion detected by pumping messages and checking every ≤2 ms.
