# S15 - the system's own web view inside our window (opened 2026-11-15)

**Status: VERDICT (macOS): PASS (2026-11-15); Windows: not run (the code is written and type-checks, nothing more). The
system's web view sits as a child in a rectangle of a winit 0.31 window drawn with wgpu / vello, through the `wry`
crate, with two pieces of our own on macOS. No engine is bundled. Six rules for the product came out of it (below);
two of them are traps that the library's defaults walk into.**

## Why

UFB's full-window preview (the lightbox) must show HTML files - above all the pages and "web folders" the notes
application exports, which carry video players and scripts. The Qt application bundled Chromium. The owner's ruling:
"Native webview or nothing, we don't want to package a full chromium or anything like that." That leaves WKWebView on
macOS and WebView2 on Windows, and the family has never put a system web view inside one of its windows.

## The question

Can the system's own web view be shown as a CHILD inside a rectangular region of a winit 0.31 window whose content we
draw with wgpu / vello, on macOS and on Windows?

## Pass bar

1. The view is inside a given rectangle of our window (not a window of its own), shows a LOCAL page the spike
   generates, and our drawing stays around it: a strip above with Close, a strip below with a path.
2. The page's script runs (the host reads something back) and a `<video>` with a tiny generated clip is live.
3. The rectangle is changed 50 times by script and the view follows, no crash, no stray frame; removing the view gives
   the region back to our drawing; 20 lives with no window or process left behind.
4. A link to an outside address is NOT loaded: the host is told. The page reads files in its own folder and not
   outside it - and what exactly the OS API allows is reported.
5. Keys and pointer: who has the keys, how Escape / Space still reach the lightbox, whether our controls beside the
   view still get the pointer - by the APIs and by what can be observed without sending input.
6. Proof by picture: `screencapture -l <window>` of the spike's own window, page pixels inside the rectangle.
7. What it costs; for Windows, how the runtime's absence is detected and what the program does then.

## Time box

About an hour. Used: about an hour and a half (the message channel, see "Crate decision", was the overrun).

## How to run

`CARGO_TARGET_DIR=target/s15 cargo build --release -p s15-webview`, then `S15_SCRIPT=all target/s15/release/s15`: one
window for about 22 s, no input sent, findings printed and written to `target/s15-out/result.txt`, three pictures
beside it. With no `S15_SCRIPT` the window stays for a walk by hand (Close removes the view and makes it again; keys
that reach OUR window are printed). `--no-default-features` builds the same program with no web view.

## Results (macOS 27.2, Apple silicon, a 2x screen; the finished script ran 5 times, 18 checks, 18 passed each time)

| | Result |
|---|---|
| 1. Inside the rectangle, our drawing around it | yes. Picture 1: page pixels on 100% of the bands just inside the rectangle's four edges, 0.0% just outside, none in either strip |
| 2. Script | the host read back the title the script computed (`s15-title-42`) and its posted messages |
| 2. `<video>` | live: `currentTime` moved in 7 of 7 samples 400 ms apart, `videoWidth` 160 (H.264 in MP4, 9 KB, autoplay, muted). A `<canvas>` animation ran 59 frames in its first second |
| 3. 50 rectangles (one every 33 ms) | followed; picture 2: the page exactly in the last one, 0.0% page pixels outside it, 100% our drawing where it had been |
| 3. Removed | picture 3: no page pixel left, 100% our drawing in the region |
| 3. 20 lives | 20 of 20 ran their script; 0 child views left; the application's windows 2 before and 2 after (ours and a hidden text-input window of the system's that is there before any web view); see "Processes" |
| 4. Outside link | `https://example.com/` was told to the host and not loaded; the view stayed on its page. `window.open()` of an outside address: told and refused (the page gets `null`). Nothing was fetched from the network |
| 4. Files | see "File access" |
| 5. Keys and pointer | see "Focus and keyboard" |
| 6. Pictures | `target/s15-out/shot-1-open.png`, `shot-2-resized.png`, `shot-3-removed.png`, by `screencapture -x -o -l` |
| 7. Cost | see "Cost" |

One thing about the pictures: on the very first run (a spike still with bugs) all three `screencapture -l` calls
answered "could not create image from window"; on every later run they worked, with the capture unchanged. The cause
was not found. The program has a fallback (capturing its own window through CoreGraphics) that was therefore never
exercised.

### Processes

A WKWebView is three helper processes of the system's, not children of ours: one **WebContent** per view, and one
**GPU** and one **Networking** process for the whole application. Measured: 0 before the first view; 3 with it; the
WebContent process is gone as soon as its view is dropped (each of the 20 lives had a different one, none was left);
GPU and Networking stay for the life of the application once a view has existed (still there 3 s after the last view)
and end with it (the machine's count of WebKit processes was the same before and after every run).

### Cost

- Crate: `wry` 0.57.2 (MIT OR Apache-2.0), default features off, `os-webview` on (needed for the runtime check).
- Binary, macOS release: **+441 KB** (14,114,808 against 13,673,416 bytes; stripped +265 KB). WebKit is the system's
  framework.
- New crates in the tree on macOS: `wry`, `http`, `url` (+ `idna`, `form_urlencoded`, `percent-encoding`), `cookie`
  (+ `time`), `dirs` (+ `option-ext`, MPL-2.0 - compatible with GPL-3.0-or-later), `bytes`, `objc2-exception-helper`;
  `objc2-web-kit` 0.3.2 (the version wry uses). On Windows: `wry`, `webview2-com` 0.39.1 (MIT; on `windows` 0.62, the
  workspace's version), `http`, `cookie`, `dunce`, `windows-version`. All MIT / Apache / Zlib but `option-ext`.
  `webview2-com-sys` links Microsoft's `WebView2LoaderStatic.lib` into the executable: its licence (Microsoft's
  BSD-style SDK licence) is to be confirmed before a release.
- The first web view: the call returns in **39-51 ms**; the page's script has run and its first message is with the
  host **157-237 ms** after the view was asked for (253 ms to "load finished" on the first run of the session). Later
  views: 20 ms to make.

## Crate decision: `wry`, with two pieces of our own on macOS

`WebViewBuilder::build_as_child(&window)` takes winit 0.31's window as it is (`Arc<Box<dyn Window>>` has a window
handle) and gives a child with bounds, a navigation handler, a new-window handler, a title handler and a script
channel - on both systems with one code path. Calling WKWebView directly through `objc2-web-kit` is small (a
configuration, two delegates, one message handler: some 150 lines), but the same thing on Windows through
`webview2-com` is not: the environment and the controller are made through callbacks while messages are pumped, plus
the child window, its DPI, its focus and its teardown order. wry has that solved and it is the half we cannot run
here. So: wry for the machinery.

What wry does NOT do right for our case on macOS, both worked around in the spike through the handles wry gives out
(`WebViewExtMacOS::webview()` and `manager()`):

1. **Loading a file.** wry loads a `file://` address with `loadRequest:`, which is not confined (see "File access").
   The spike makes the view with no address and calls `loadFileURL:allowingReadAccessToURL:` itself.
2. **Messages from a file page are dropped.** wry's `window.ipc.postMessage` builds an `http::Uri` from the page's
   address before calling the handler; `file:///...` has no authority, the build fails and the message is discarded
   with no error (first run: title and navigation arrived, no message did). The spike adds its own
   `WKScriptMessageHandler` (25 lines) to wry's content controller. Worth reporting upstream.

Also to know: wry turns on `objc2`'s `exception` and `disable-encoding-assertions` features, and cargo unifies
features, so every application that links wry loses objc2's encoding assertions in its debug builds.

If those two pieces grow, the fallback is plain: `objc2-web-kit` directly on macOS and wry only on Windows.

## File access - what the OS allows

macOS, measured (an image beside the page; an image and a text file in a sibling folder):

| How the page is loaded | image beside | image in a sibling folder | `fetch()` of either |
|---|---|---|---|
| `loadFileURL:` page `allowingReadAccessToURL:` **the page's own folder** | loads | **fails** | fails |
| the same with the folder ABOVE | loads | loads | fails |
| wry's own `file://` load (`loadRequest:`) | loads | **loads** | fails |

- `allowingReadAccessToURL:` IS the control: the web content process can read that folder (and below) and nothing
  else. For a "web folder" it is the folder's root; for a single page, the folder it lies in.
- `fetch()` / XHR of a `file://` address fails everywhere, own folder included (a file page has an opaque origin), as
  in any browser. A page that works when opened from disk in Safari works here; one that needs `fetch` needs a server
  or a custom scheme (not tested; a custom scheme must answer range requests for video).
- The read access says nothing about the NETWORK: a page may still name `https://` images, scripts or frames, and
  they would be fetched. The navigation handler only refuses navigations. Not tested; a ruling is needed (below).

Windows, by the API (not run):

- A `file://` navigation works with no setting, and a file page can show any other file by its path (`<img>`,
  `<video>`); only script reads are blocked. That is the unconfined case.
- `ICoreWebView2_3::SetVirtualHostNameToFolderMapping(host, folder, DENY_CORS)` maps ONE folder to a made-up https
  host; the page is opened as `https://<host>/index.html` and nothing else on the disk has an address it can name.
  The page then has a real origin, so `fetch` of its own folder works too. This is what the spike's Windows code
  does. Microsoft's documentation warns that media served through such a mapping can be slow to load: the first
  Windows run must time a large video; the alternatives are a custom scheme with range answers
  (`WebResourceRequested`) or the unconfined `file://`.

## Focus and keyboard

Observed on macOS without input (the window's first responder, hit tests), and read from the APIs:

- **Pointer.** A click on Close or in the bottom strip goes to winit's view; a click inside the rectangle goes to the
  web view. Our controls beside it keep working; nothing we draw can be clicked INSIDE the rectangle.
- **Made unfocused, the view does not take the keys**: they stay with winit's view. wry's default is `focused = true`,
  which for a child on macOS does not move the keys either but ACTIVATES the application - always pass
  `with_focused(false)`. (On Windows `focused` does move the focus into the view.)
- **A click in the page gives it the keys** (stood in for by `focus()`: first responder = the web view). From then on
  Escape and Space go to the page, not to winit. Two ways back, both shown:
  - the host's own script, added to every page before the page's (`with_initialization_script`): a capturing
    `keydown` listener posts Escape to the host. Shown with a DOM event made by script (no key was sent): the host got
    `key:Escape`. Space is better left to the page while it has the keys (it plays a video); the lightbox takes it
    when our view has them;
  - `focus_parent()` gives the keys back to winit's view (shown) - for when the pointer goes back to our strips.
- **Trap: removing the view while it has the keys leaves them with the WINDOW, not with winit's view** (first
  responder = the window; shown). winit's view then gets no key until it is clicked. The lightbox calls
  `focus_parent()` BEFORE it drops the view.
- wry's child view answers `performKeyEquivalent:` with "no", so Cmd shortcuts fall through to the window and the
  menu - and are never seen by the page.
- Windows, by the API: the view is another process's window; with the focus in it our window gets no key messages.
  The same host script carries Escape; the controller's `AcceleratorKeyPressed` (always raised for Escape) is the
  second way; `focus_parent()` is `SetFocus` on our window.
- NOT observable without input, left for the owner's walk: a real click, real Escape / Space, the page's own context
  menu, a file dropped on the view, scrolling.

## What the real lightbox must do

1. Make the view with `with_focused(false)`, bounds in LOGICAL units, and set the bounds again at every resize and
   scale change in the same turn as our redraw.
2. macOS: load with `loadFileURL:allowingReadAccessToURL:` (the page's folder, or the web folder's root) - never the
   library's `file://` load. Windows: the folder mapping (or what the first run decides).
3. Allow navigations only to addresses under that folder; everything else goes to the host, which opens the system
   browser; refuse new windows. (The spike's handler allows any `file://`; the product compares against the folder -
   a dropped file must not navigate the view elsewhere.)
4. Carry Escape through the host's script and its own message handler (macOS: ours, not `window.ipc`); call
   `focus_parent()` before dropping the view and when the pointer returns to our controls.
5. **Nothing of ours can be drawn over the view**: it is a system view above our surface. A menu, a tooltip, a
   dialog or a toast that would overlap the rectangle needs the view hidden or moved first. The window's own capture
   (`Presenter::capture_window`, the shots harness) does not contain the page.
6. Drop the view when the lightbox closes or moves to another file (that ends its content process and its sound);
   expect two system processes (GPU, Networking) to stay for the application's life.

Rulings needed before it is built:

- May a previewed page reach the network (remote images, scripts, frames)? [proposed: no - a content rule list on
  macOS and a request filter on Windows; to be spiked with the Windows run]
- The page's own context menu (Reload, Back...) [proposed: off].

## The Windows plan, and a missing runtime

- The code is in the spike under `cfg(windows)`: the same `wry` builder, the folder mapping through
  `WebViewExtWindows::webview()`, observation of the focus, of child windows and of `msedgewebview2.exe` processes
  descending from ours. `cargo check --target x86_64-pc-windows-msvc` passes from macOS; it has never run.
- **WebView2 keeps a profile folder, by default beside the executable** - not writable for an installed or packaged
  program. A `WebContext` with a folder under the user's local application data is required (the spike uses its
  output folder).
- To see on the first run: the child window against our DX12 swap chain (no flicker at a resize, nothing left behind
  when removed), video through the folder mapping, the focus, the processes after 20 lives (a browser process and its
  children per profile folder; how long they outlive the last view), the picture by eye (no `screencapture` there).
- **Detecting the runtime:** `wry::webview_version()` (WebView2's `GetAvailableCoreWebView2BrowserVersionString`)
  answers with a version or an error. Windows 11 ships the runtime; Windows 10 has it on most machines (it comes
  with Edge's updates) but not by guarantee.
- **When it is missing** [proposed]: the lightbox shows its ordinary "no preview" card for the file with "Open in
  browser"; UFB neither installs nor downloads the runtime, and says once in its log that it is absent. Checked once
  at the first HTML preview, not at start.

## VERDICT (macOS): PASS; Windows: not run

On macOS every point of the bar is met by script and by picture, five runs out of five, with two workarounds for the
library on file pages and one focus rule. On Windows the same code path is written and type-checked only; the verdict
there waits for the owner's run.
