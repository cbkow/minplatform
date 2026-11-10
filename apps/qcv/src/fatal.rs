//! The one failure the app cannot draw its way out of: no usable graphics device (canon row 250). A machine with no GPU
//! behind the window — a virtual machine, a remote session — either has no adapter or loses its device at the first
//! present; the window would sit blank. The app says so and exits. On Windows the message is its own small DARK window
//! (plain Win32 + GDI, the theme's greys: nothing here may need the GPU, and a system message box is white), on its own
//! thread so its message loop never runs the main window's handlers. Elsewhere: the log and stderr.
use min_i18n::trc;

pub fn no_graphics(detail: &str) -> ! {
    log::error!("no usable graphics device: {detail}");
    eprintln!("QCView: no usable graphics device: {detail}");
    // the copy (the user 2026-11-09, "a bit verbose" → heading + one line)
    let head = trc("Main", "No compatible graphics card");
    let body = trc("Main", "QCView needs DirectX 12 and a current graphics driver.");
    let close = trc("Main", "Close");
    #[cfg(windows)] { let _ = std::thread::spawn(move || win::show(head, body, close)).join(); }
    #[cfg(not(windows))] { let _ = (head, body, close); }
    std::process::exit(1)
}

#[cfg(windows)]
mod win {
    use min_theme::tokens as t;
    use std::sync::Mutex;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::*;

    struct State { head: Vec<u16>, body: Vec<u16>, close: Vec<u16>, scale: f64, button: RECT }
    static STATE: Mutex<Option<State>> = Mutex::new(None);
    fn rgb(c: min_theme::Color) -> COLORREF { COLORREF(((c.r * 255.0).round() as u32) | (((c.g * 255.0).round() as u32) << 8) | (((c.b * 255.0).round() as u32) << 16)) }
    fn px(v: f64, s: f64) -> i32 { (v * s).round() as i32 }

    pub fn show(head: &str, body: &str, close: &str) {
        unsafe {
            let scale = GetDpiForSystem() as f64 / 96.0;
            let (w_px, h_px) = (px(440.0, scale), px(176.0, scale));
            *STATE.lock().unwrap() = Some(State { head: head.encode_utf16().collect(), body: body.encode_utf16().collect(), close: close.encode_utf16().collect(), scale, button: RECT::default() });
            let inst = GetModuleHandleW(PCWSTR::null()).map(|h| windows::Win32::Foundation::HINSTANCE(h.0)).unwrap_or_default();
            let class = w!("QCViewFatal");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst, lpszClassName: class, hbrBackground: CreateSolidBrush(rgb(t::RAIL_BACKGROUND_COLOR)), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
            RegisterClassW(&wc);
            let (sx, sy) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("QCView"), WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, (sx - w_px) / 2, (sy - h_px) / 2, w_px, h_px, None, None, Some(inst), None) else { return };
            // the caption: dark, and on Windows 11 the window's own grey
            let on = windows::core::BOOL(1);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as *const _, std::mem::size_of_val(&on) as u32);
            let cap = rgb(t::RAIL_BACKGROUND_COLOR);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &cap as *const _ as *const _, std::mem::size_of_val(&cap) as u32);
            // the two icons, as the main window's: the title bar's is the macOS icon, the taskbar's the exe's own (resource 1)
            if let Some((mut px_, w, h)) = crate::win_icon::rgba() {
                for p in px_.chunks_exact_mut(4) { p.swap(0, 2); }   // RGBA → BGRA
                let mask = vec![0u8; (w * h / 8) as usize];
                if let Ok(ic) = CreateIcon(Some(inst), w as i32, h as i32, 1, 32, mask.as_ptr(), px_.as_ptr()) { SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(ic.0 as isize))); }
            }
            if let Ok(ic) = LoadIconW(Some(inst), PCWSTR(1 as *const u16)) { SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(ic.0 as isize))); }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let dc = BeginPaint(hwnd, &mut ps);
                    let mut client = RECT::default(); let _ = GetClientRect(hwnd, &mut client);
                    if let Some(st) = STATE.lock().unwrap().as_mut() {
                        let s = st.scale; let pad = px(20.0, s);
                        let mk = |size: f64, weight: i32| CreateFontW(-px(size, s), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
                        let (hfont, font) = (mk(17.0, 600), mk(14.0, 400));
                        SetBkMode(dc, TRANSPARENT);
                        let old = SelectObject(dc, HGDIOBJ(hfont.0));
                        SetTextColor(dc, rgb(t::MENU_BAR_TITLE_COLOR_OPEN));
                        let mut hr = RECT { left: pad, top: pad, right: client.right - pad, bottom: pad + px(26.0, s) };
                        DrawTextW(dc, &mut st.head, &mut hr, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
                        SelectObject(dc, HGDIOBJ(font.0));
                        SetTextColor(dc, rgb(t::MENU_BAR_TITLE_COLOR));
                        let mut text = RECT { left: pad, top: pad + px(32.0, s), right: client.right - pad, bottom: client.bottom - px(60.0, s) };
                        DrawTextW(dc, &mut st.body, &mut text, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
                        // the Close button: a rounded grey block, bottom right
                        let (bw, bh) = (px(96.0, s), px(32.0, s));
                        st.button = RECT { left: client.right - pad - bw, top: client.bottom - pad - bh, right: client.right - pad, bottom: client.bottom - pad };
                        let brush = CreateSolidBrush(rgb(t::HOVER_BACKGROUND_COLOR));
                        let (ob, op) = (SelectObject(dc, HGDIOBJ(brush.0)), SelectObject(dc, GetStockObject(NULL_PEN)));
                        let _ = RoundRect(dc, st.button.left, st.button.top, st.button.right, st.button.bottom, px(8.0, s), px(8.0, s));
                        SelectObject(dc, ob); SelectObject(dc, op); let _ = DeleteObject(HGDIOBJ(brush.0));
                        SetTextColor(dc, rgb(t::MENU_BAR_TITLE_COLOR_OPEN));
                        let mut b = st.button;
                        DrawTextW(dc, &mut st.close, &mut b, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                        SelectObject(dc, old); let _ = DeleteObject(HGDIOBJ(font.0)); let _ = DeleteObject(HGDIOBJ(hfont.0));
                    }
                    let _ = EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                WM_LBUTTONUP => {
                    let (x, y) = ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32);
                    let hit = STATE.lock().unwrap().as_ref().is_some_and(|st| x >= st.button.left && x < st.button.right && y >= st.button.top && y < st.button.bottom);
                    if hit { let _ = DestroyWindow(hwnd); }
                    LRESULT(0)
                }
                WM_KEYDOWN if wp.0 == 0x0D || wp.0 == 0x1B || wp.0 == 0x20 => { let _ = DestroyWindow(hwnd); LRESULT(0) }   // Enter, Escape, Space
                WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }
}
