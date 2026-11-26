#![windows_subsystem = "windows"]

mod scanner;

use scanner::ScreenSnapshot;
use std::cell::RefCell;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, GetStockObject,
    InvalidateRect, Rectangle, ReleaseDC, SelectObject, SetBkMode, SetDIBitsToDevice,
    SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DT_CALCRECT, DT_WORDBREAK,
    HBRUSH, HFONT, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetMessageW, GetSystemMetrics, LoadCursorW, LoadIconW,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, TrackPopupMenu, TranslateMessage, BS_PUSHBUTTON, CW_USEDEFAULT,
    ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, IDC_HAND,
    MF_CHECKED, MF_POPUP, MF_STRING, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOZORDER, SW_SHOW,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC,
    WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_TOPMOST,
    WS_EX_TOOLWINDOW, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_VISIBLE, WS_VSCROLL, SW_HIDE,
    SetCursor, GetDlgCtrlID,
};

const WM_TRAYICON: u32 = WM_APP + 1;
const WM_APP_UPDATE_FOUND: u32 = WM_APP + 2;
const SS_CENTER: u32 = 0x0000_0001;
const SS_BITMAP: u32 = 0x0000_000E;
const SS_NOTIFY: u32 = 0x0000_0100;
const STM_SETIMAGE: u32 = 0x0172;
const IMAGE_BITMAP: usize = 0;

const IDM_ABOUT: usize = 1001;
const IDM_EXIT: usize = 1002;
const IDM_LANG_EN: usize = 1003;
const IDM_LANG_ZH: usize = 1004;

const IDC_RESULT_EDIT: isize = 2001;
const IDC_BTN_COPY: isize = 2002;
const IDC_BTN_OPEN_URL: isize = 2003;
const IDC_BTN_RESCAN: isize = 2004;
const IDC_BTN_CLOSE: isize = 2005;
const IDC_LINK_GITHUB: isize = 2006;
const IDC_LINK_UPDATE: isize = 2007;
const IDC_BTN_UPDATE_GO: isize = 2008;
const IDC_BTN_UPDATE_LATER: isize = 2009;
const IDC_LINK_KOFI: isize = 2010;

#[derive(Clone)]
struct UpdateInfo {
    version: String,
    url: String,
}

struct OverlayState {
    snapshot: ScreenSnapshot,
    bgra_pixels: Vec<u8>,
    dimmed_bgra_pixels: Vec<u8>,
    dragging: bool,
    start: POINT,
    current: POINT,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    En,
    Zh,
}

fn current_lang() -> Lang {
    APP.with(|app| {
        app.borrow()
            .as_ref()
            .map(|st| st.lang)
            .unwrap_or(Lang::En)
    })
}

fn tr<'a>(en: &'a str, zh: &'a str) -> &'a str {
    match current_lang() {
        Lang::En => en,
        Lang::Zh => zh,
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("qr-scanner.json")))
}

fn load_lang() -> Lang {
    let Some(path) = config_path() else {
        return Lang::En;
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return Lang::En;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Lang::En;
    };
    match val.get("lang").and_then(|v| v.as_str()) {
        Some("zh") => Lang::Zh,
        _ => Lang::En,
    }
}

fn save_lang(lang: Lang) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let code = match lang {
        Lang::En => "en",
        Lang::Zh => "zh",
    };
    if let Ok(json) = serde_json::to_string_pretty(&serde_json::json!({ "lang": code })) {
        let _ = std::fs::write(path, json);
    }
}

struct AppState {
    tray_hwnd: HWND,
    overlay_hwnd: HWND,
    result_hwnd: HWND,
    about_hwnd: HWND,
    update_hwnd: HWND,
    font: HFONT,
    link_font: HFONT,
    overlay: Option<OverlayState>,
    decoded_results: Vec<String>,
    update_info: Arc<Mutex<Option<UpdateInfo>>>,
    lang: Lang,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = clean.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_str = parts.next()?.split('-').next()?;
    let patch = patch_str.parse().ok()?;
    Some((major, minor, patch))
}

fn check_github_update() -> Option<UpdateInfo> {
    let resp = ureq::get("https://api.github.com/repos/zungmou/qr-scanner/releases/latest")
        .set("User-Agent", "qr-scanner")
        .timeout(Duration::from_secs(5))
        .call()
        .ok()?;
    let body = resp.into_string().ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let html_url = json.get("html_url")?.as_str()?;

    let current = parse_semver(env!("CARGO_PKG_VERSION"))?;
    let latest = parse_semver(tag)?;
    if latest > current {
        Some(UpdateInfo {
            version: tag.trim_start_matches('v').trim_start_matches('V').to_string(),
            url: html_url.to_string(),
        })
    } else {
        None
    }
}

unsafe fn create_ui_font(underline: bool) -> HFONT {
    let face = to_wide("Microsoft YaHei UI");
    CreateFontW(
        -15,
        0,
        0,
        0,
        400,
        0,
        if underline { 1 } else { 0 },
        0,
        1,
        0,
        0,
        0,
        0,
        face.as_ptr(),
    )
}

unsafe fn set_control_font(hwnd: HWND, font: HFONT) {
    SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1);
}

unsafe fn center_window_pos(w: i32, h: i32) -> (i32, i32) {
    let sw = GetSystemMetrics(SM_CXSCREEN);
    let sh = GetSystemMetrics(SM_CYSCREEN);
    ((sw - w) / 2, (sh - h) / 2)
}

unsafe fn start_capture() {
    let hinstance = GetModuleHandleW(null());
    let snap = match scanner::capture_primary_screen() {
        Ok(s) => s,
        Err(e) => {
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    st.decoded_results = vec![match current_lang() {
                        Lang::En => format!("Screen capture failed: {e}"),
                        Lang::Zh => format!("截屏失败: {e}"),
                    }];
                }
            });
            show_result_window();
            return;
        }
    };

    // Convert RGBA to top-down BGRA (bright & dimmed) for Win32 SetDIBitsToDevice
    let raw = snap.image.as_raw();
    let mut bgra = Vec::with_capacity(raw.len());
    let mut dimmed_bgra = Vec::with_capacity(raw.len());
    for chunk in raw.chunks_exact(4) {
        let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        bgra.push(b);
        bgra.push(g);
        bgra.push(r);
        bgra.push(a);

        dimmed_bgra.push(((b as u16 * 115) / 256) as u8);
        dimmed_bgra.push(((g as u16 * 115) / 256) as u8);
        dimmed_bgra.push(((r as u16 * 115) / 256) as u8);
        dimmed_bgra.push(a);
    }

    let x = snap.origin_x;
    let y = snap.origin_y;
    let w = snap.width as i32;
    let h = snap.height as i32;

    APP.with(|app| {
        if let Some(ref mut st) = *app.borrow_mut() {
            if !st.overlay_hwnd.is_null() {
                DestroyWindow(st.overlay_hwnd);
                st.overlay_hwnd = null_mut();
            }
            st.overlay = Some(OverlayState {
                snapshot: snap,
                bgra_pixels: bgra,
                dimmed_bgra_pixels: dimmed_bgra,
                dragging: false,
                start: POINT { x: 0, y: 0 },
                current: POINT { x: 0, y: 0 },
            });
        }
    });

    let class_name = to_wide("QrScannerOverlayClass");
    let title = to_wide("QR Scanner Overlay");
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_POPUP | WS_VISIBLE,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        hinstance,
        null(),
    );

    APP.with(|app| {
        if let Some(ref mut st) = *app.borrow_mut() {
            st.overlay_hwnd = hwnd;
        }
    });

    SetForegroundWindow(hwnd);
}

unsafe fn show_result_window() {
    let hinstance = GetModuleHandleW(null());
    let (existing, results, font) = APP.with(|app| {
        let st = app.borrow();
        let st = st.as_ref().unwrap();
        (st.result_hwnd, st.decoded_results.clone(), st.font)
    });

    if !existing.is_null() {
        DestroyWindow(existing);
    }

    let w = 480;
    let h = 280;
    let (x, y) = center_window_pos(w, h);
    let class_name = to_wide("QrScannerResultClass");
    let title = to_wide(tr("QR Code Result", "二维码识别结果"));
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        hinstance,
        null(),
    );

    let text = if results.is_empty() {
        tr(
            "No QR code or barcode detected in the selected area.",
            "未在框选区域内检测到二维码或条码。",
        )
        .to_string()
    } else {
        results.join("\r\n\r\n")
    };
    let is_url = results
        .first()
        .map(|s| s.starts_with("http://") || s.starts_with("https://"))
        .unwrap_or(false);

    let edit_cls = to_wide("EDIT");
    let text_w = to_wide(&text);
    let edit_hwnd = CreateWindowExW(
        0,
        edit_cls.as_ptr(),
        text_w.as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32,
        16,
        16,
        432,
        165,
        hwnd,
        IDC_RESULT_EDIT as _,
        hinstance,
        null(),
    );
    set_control_font(edit_hwnd, font);

    let btn_cls = to_wide("BUTTON");
    let mut bx = 16;
    if !results.is_empty() {
        let copy_txt = to_wide(tr("Copy", "复制结果"));
        let b1 = CreateWindowExW(
            0,
            btn_cls.as_ptr(),
            copy_txt.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
            bx,
            195,
            100,
            32,
            hwnd,
            IDC_BTN_COPY as _,
            hinstance,
            null(),
        );
        set_control_font(b1, font);
        bx += 110;

        if is_url {
            let open_txt = to_wide(tr("Open URL", "打开链接"));
            let b2 = CreateWindowExW(
                0,
                btn_cls.as_ptr(),
                open_txt.as_ptr(),
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
                bx,
                195,
                100,
                32,
                hwnd,
                IDC_BTN_OPEN_URL as _,
                hinstance,
                null(),
            );
            set_control_font(b2, font);
            bx += 110;
        }
    }

    let rescan_txt = to_wide(tr("Rescan", "重新框选"));
    let b3 = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        rescan_txt.as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
        bx,
        195,
        100,
        32,
        hwnd,
        IDC_BTN_RESCAN as _,
        hinstance,
        null(),
    );
    set_control_font(b3, font);

    APP.with(|app| {
        if let Some(ref mut st) = *app.borrow_mut() {
            st.result_hwnd = hwnd;
        }
    });

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe fn measure_text_height(font: HFONT, text: &[u16], max_width: i32) -> i32 {
    let hdc = GetDC(null_mut());
    let old_font = SelectObject(hdc, font);
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: max_width,
        bottom: 0,
    };
    DrawTextW(hdc, text.as_ptr(), -1, &mut rc, DT_CALCRECT | DT_WORDBREAK);
    SelectObject(hdc, old_font);
    ReleaseDC(null_mut(), hdc);
    (rc.bottom - rc.top).max(20)
}

unsafe fn show_about_window() {
    let hinstance = GetModuleHandleW(null());
    let (existing, font, link_font, update_info) = APP.with(|app| {
        let st = app.borrow();
        let st = st.as_ref().unwrap();
        let info = st.update_info.lock().ok().and_then(|g| g.clone());
        (st.about_hwnd, st.font, st.link_font, info)
    });

    if !existing.is_null() {
        SetForegroundWindow(existing);
        return;
    }

    let desc = to_wide(tr(
        "A lightweight Windows system-tray screen QR & barcode scanner",
        "轻量级 Windows 系统托盘屏幕二维码与条码扫描工具",
    ));
    let desc_h = measure_text_height(font, &desc, 364);
    let kofi_tip = to_wide(tr(
        "If you find this helpful, scan or click to buy me a coffee:",
        "如果这个工具对你有帮助，欢迎扫码或点击支持：",
    ));
    let kofi_tip_h = measure_text_height(font, &kofi_tip, 364);
    let w = 420;
    let base_h = if update_info.is_some() { 386 } else { 358 };
    let h = base_h + (desc_h - 20) + (kofi_tip_h - 20);
    let (x, y) = center_window_pos(w, h);
    let class_name = to_wide("QrScannerAboutClass");
    let title = to_wide(tr("About QR Scanner (Beta)", "关于 QR Scanner (Beta)"));
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        hinstance,
        null(),
    );

    let static_cls = to_wide("STATIC");
    let mut cy = 18;

    let h1 = to_wide(&format!(
        "QR Scanner (Beta)  v{}",
        env!("CARGO_PKG_VERSION")
    ));
    let lbl1 = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        h1.as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_CENTER,
        20,
        cy,
        364,
        24,
        hwnd,
        null_mut(),
        hinstance,
        null(),
    );
    set_control_font(lbl1, font);
    cy += 30;

    if let Some(info) = update_info {
        let upd_txt = to_wide(&match current_lang() {
            Lang::En => format!("New version v{} available, click to download", info.version),
            Lang::Zh => format!("发现新版本 v{}，点击前往下载", info.version),
        });
        let lbl_upd = CreateWindowExW(
            0,
            static_cls.as_ptr(),
            upd_txt.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER | SS_NOTIFY,
            20,
            cy,
            364,
            22,
            hwnd,
            IDC_LINK_UPDATE as _,
            hinstance,
            null(),
        );
        set_control_font(lbl_upd, link_font);
        cy += 28;
    }

    let lbl2 = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        desc.as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_CENTER,
        20,
        cy,
        364,
        desc_h,
        hwnd,
        null_mut(),
        hinstance,
        null(),
    );
    set_control_font(lbl2, font);
    cy += desc_h + 8;

    let gh = to_wide("https://github.com/zungmou/qr-scanner");
    let lbl_gh = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        gh.as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_CENTER | SS_NOTIFY,
        20,
        cy,
        364,
        22,
        hwnd,
        IDC_LINK_GITHUB as _,
        hinstance,
        null(),
    );
    set_control_font(lbl_gh, link_font);
    cy += 30;

    let lbl_kofi_tip = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        kofi_tip.as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_CENTER,
        20,
        cy,
        364,
        kofi_tip_h,
        hwnd,
        null_mut(),
        hinstance,
        null(),
    );
    set_control_font(lbl_kofi_tip, font);
    cy += kofi_tip_h + 8;

    let qr_size = 120i32;
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/kofi-qr.png")) {
        let resized = img.resize_exact(
            qr_size as u32,
            qr_size as u32,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba = resized.to_rgba8();
        let mut bgra = Vec::with_capacity(rgba.as_raw().len());
        for px in rgba.as_raw().chunks_exact(4) {
            bgra.push(px[2]);
            bgra.push(px[1]);
            bgra.push(px[0]);
            bgra.push(px[3]);
        }
        let hbmp = CreateBitmap(qr_size, qr_size, 1, 32, bgra.as_ptr() as *const _);
        let lbl_kofi = CreateWindowExW(
            0,
            static_cls.as_ptr(),
            null(),
            WS_CHILD | WS_VISIBLE | SS_BITMAP | SS_NOTIFY,
            (w - 16 - qr_size) / 2,
            cy,
            qr_size,
            qr_size,
            hwnd,
            IDC_LINK_KOFI as _,
            hinstance,
            null(),
        );
        SendMessageW(lbl_kofi, STM_SETIMAGE, IMAGE_BITMAP, hbmp as isize);
    }
    cy += qr_size + 14;

    let btn_cls = to_wide("BUTTON");
    let close_txt = to_wide(tr("Close", "关闭"));
    let btn = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        close_txt.as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
        (w - 16 - 120) / 2,
        cy,
        120,
        32,
        hwnd,
        IDC_BTN_CLOSE as _,
        hinstance,
        null(),
    );
    set_control_font(btn, font);

    APP.with(|app| {
        if let Some(ref mut st) = *app.borrow_mut() {
            st.about_hwnd = hwnd;
        }
    });

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe fn show_update_window() {
    let hinstance = GetModuleHandleW(null());
    let (existing, font, update_info) = APP.with(|app| {
        let st = app.borrow();
        let st = st.as_ref().unwrap();
        let info = st.update_info.lock().ok().and_then(|g| g.clone());
        (st.update_hwnd, st.font, info)
    });

    let Some(info) = update_info else {
        return;
    };
    if !existing.is_null() {
        SetForegroundWindow(existing);
        return;
    }

    let w = 380;
    let h = 190;
    let (x, y) = center_window_pos(w, h);
    let class_name = to_wide("QrScannerUpdateClass");
    let title = to_wide(tr("Update Available", "发现新版本"));
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        hinstance,
        null(),
    );

    let static_cls = to_wide("STATIC");
    let msg = to_wide(&match current_lang() {
        Lang::En => format!(
            "New version v{} is available (current: v{})",
            info.version,
            env!("CARGO_PKG_VERSION")
        ),
        Lang::Zh => format!(
            "检测到新版本 v{}（当前版本 v{}）",
            info.version,
            env!("CARGO_PKG_VERSION")
        ),
    });
    let lbl = CreateWindowExW(
        0,
        static_cls.as_ptr(),
        msg.as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_CENTER,
        20,
        34,
        324,
        24,
        hwnd,
        null_mut(),
        hinstance,
        null(),
    );
    set_control_font(lbl, font);

    let btn_cls = to_wide("BUTTON");
    let go_txt = to_wide(tr("Download on GitHub", "前往 GitHub 下载"));
    let b1 = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        go_txt.as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
        56,
        88,
        136,
        32,
        hwnd,
        IDC_BTN_UPDATE_GO as _,
        hinstance,
        null(),
    );
    set_control_font(b1, font);

    let later_txt = to_wide(tr("Later", "稍后再说"));
    let b2 = CreateWindowExW(
        0,
        btn_cls.as_ptr(),
        later_txt.as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32,
        204,
        88,
        104,
        32,
        hwnd,
        IDC_BTN_UPDATE_LATER as _,
        hinstance,
        null(),
    );
    set_control_font(b2, font);

    APP.with(|app| {
        if let Some(ref mut st) = *app.borrow_mut() {
            st.update_hwnd = hwnd;
        }
    });

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (lparam & 0xFFFF) as u32;
            if event == WM_LBUTTONUP {
                start_capture();
            } else if event == WM_RBUTTONUP {
                let lang = current_lang();
                let hmenu = CreatePopupMenu();
                let hlang_menu = CreatePopupMenu();

                let en_txt = to_wide("English");
                let zh_txt = to_wide("简体中文");
                AppendMenuW(
                    hlang_menu,
                    MF_STRING | if lang == Lang::En { MF_CHECKED } else { 0 },
                    IDM_LANG_EN,
                    en_txt.as_ptr(),
                );
                AppendMenuW(
                    hlang_menu,
                    MF_STRING | if lang == Lang::Zh { MF_CHECKED } else { 0 },
                    IDM_LANG_ZH,
                    zh_txt.as_ptr(),
                );

                let lang_txt = to_wide(tr("Language", "语言"));
                let about_txt = to_wide(tr("About", "关于"));
                let exit_txt = to_wide(tr("Exit", "退出"));
                AppendMenuW(hmenu, MF_POPUP, hlang_menu as usize, lang_txt.as_ptr());
                AppendMenuW(hmenu, MF_STRING, IDM_ABOUT, about_txt.as_ptr());
                AppendMenuW(hmenu, MF_STRING, IDM_EXIT, exit_txt.as_ptr());
                let mut pt = POINT { x: 0, y: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
                SetForegroundWindow(hwnd);
                TrackPopupMenu(
                    hmenu,
                    TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    null(),
                );
                DestroyMenu(hmenu);
            }
            0
        }
        WM_COMMAND => {
            let id = wparam & 0xFFFF;
            if id == IDM_ABOUT {
                show_about_window();
            } else if id == IDM_EXIT {
                DestroyWindow(hwnd);
            } else if id == IDM_LANG_EN || id == IDM_LANG_ZH {
                let new_lang = if id == IDM_LANG_EN { Lang::En } else { Lang::Zh };
                APP.with(|app| {
                    if let Some(st) = app.borrow_mut().as_mut() {
                        st.lang = new_lang;
                    }
                });
                save_lang(new_lang);
                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1;
                nid.uFlags = NIF_TIP;
                let tip = to_wide(match new_lang {
                    Lang::En => "QR Scanner (Click tray icon to scan)",
                    Lang::Zh => "屏幕二维码扫描 (左键点击框选)",
                });
                let copy_len = tip.len().min(nid.szTip.len());
                nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
                Shell_NotifyIconW(NIM_MODIFY, &nid);
            }
            0
        }
        WM_APP_UPDATE_FOUND => {
            show_update_window();
            0
        }
        WM_DESTROY => {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &nid);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    if let Some(ref mut ov) = st.overlay {
                        ov.dragging = true;
                        ov.start = POINT { x, y };
                        ov.current = POINT { x, y };
                    }
                }
            });
            SetCapture(hwnd);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            let mut need_repaint = false;
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    if let Some(ref mut ov) = st.overlay {
                        if ov.dragging {
                            ov.current = POINT { x, y };
                            need_repaint = true;
                        }
                    }
                }
            });
            if need_repaint {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            let mut decode_res: Option<Vec<String>> = None;
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    if let Some(ov) = st.overlay.take() {
                        let x0 = ov.start.x.min(ov.current.x).max(0) as u32;
                        let y0 = ov.start.y.min(ov.current.y).max(0) as u32;
                        let x1 = ov.start.x.max(ov.current.x).max(0) as u32;
                        let y1 = ov.start.y.max(ov.current.y).max(0) as u32;
                        let rw = x1.saturating_sub(x0);
                        let rh = y1.saturating_sub(y0);
                        if rw >= 8 && rh >= 8 {
                            decode_res = Some(scanner::decode_qr_in_region(
                                &ov.snapshot,
                                x0,
                                y0,
                                rw,
                                rh,
                            ));
                        }
                    }
                    st.overlay_hwnd = null_mut();
                }
            });
            DestroyWindow(hwnd);
            if let Some(res) = decode_res {
                APP.with(|app| {
                    if let Some(ref mut st) = *app.borrow_mut() {
                        st.decoded_results = res;
                    }
                });
                show_result_window();
            }
            0
        }
        WM_RBUTTONUP => {
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    st.overlay = None;
                    st.overlay_hwnd = null_mut();
                }
            });
            DestroyWindow(hwnd);
            0
        }
        WM_KEYDOWN => {
            if wparam == VK_ESCAPE as usize {
                APP.with(|app| {
                    if let Some(ref mut st) = *app.borrow_mut() {
                        st.overlay = None;
                        st.overlay_hwnd = null_mut();
                    }
                });
                DestroyWindow(hwnd);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            APP.with(|app| {
                if let Some(ref st) = *app.borrow() {
                    if let Some(ref ov) = st.overlay {
                        let w = ov.snapshot.width as i32;
                        let h = ov.snapshot.height as i32;
                        let mem_dc = CreateCompatibleDC(hdc);
                        let mem_bmp = CreateCompatibleBitmap(hdc, w, h);
                        let old_bmp = SelectObject(mem_dc, mem_bmp);

                        let bmi = BITMAPINFO {
                            bmiHeader: BITMAPINFOHEADER {
                                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                                biWidth: w,
                                biHeight: -h, // top-down
                                biPlanes: 1,
                                biBitCount: 32,
                                biCompression: BI_RGB,
                                biSizeImage: 0,
                                biXPelsPerMeter: 0,
                                biYPelsPerMeter: 0,
                                biClrUsed: 0,
                                biClrImportant: 0,
                            },
                            bmiColors: [std::mem::zeroed()],
                        };
                        SetDIBitsToDevice(
                            mem_dc,
                            0,
                            0,
                            w as u32,
                            h as u32,
                            0,
                            0,
                            0,
                            h as u32,
                            ov.dimmed_bgra_pixels.as_ptr() as _,
                            &bmi,
                            DIB_RGB_COLORS,
                        );

                        if ov.dragging {
                            let x0 = ov.start.x.min(ov.current.x);
                            let y0 = ov.start.y.min(ov.current.y);
                            let x1 = ov.start.x.max(ov.current.x);
                            let y1 = ov.start.y.max(ov.current.y);
                            let rw = x1 - x0;
                            let rh = y1 - y0;
                            if rw > 0 && rh > 0 {
                                let bright_dc = CreateCompatibleDC(hdc);
                                let bright_bmp = CreateCompatibleBitmap(hdc, w, h);
                                let old_bright = SelectObject(bright_dc, bright_bmp);
                                SetDIBitsToDevice(
                                    bright_dc,
                                    0,
                                    0,
                                    w as u32,
                                    h as u32,
                                    0,
                                    0,
                                    0,
                                    h as u32,
                                    ov.bgra_pixels.as_ptr() as _,
                                    &bmi,
                                    DIB_RGB_COLORS,
                                );
                                BitBlt(mem_dc, x0, y0, rw, rh, bright_dc, x0, y0, SRCCOPY);
                                SelectObject(bright_dc, old_bright);
                                DeleteObject(bright_bmp);
                                DeleteDC(bright_dc);
                            }

                            let pen = CreatePen(PS_SOLID, 2, 0x00F8BD38); // RGB(56, 189, 248)
                            let old_pen = SelectObject(mem_dc, pen);
                            let old_brush = SelectObject(mem_dc, GetStockObject(NULL_BRUSH));
                            Rectangle(mem_dc, x0, y0, x1, y1);
                            SelectObject(mem_dc, old_brush);
                            SelectObject(mem_dc, old_pen);
                            DeleteObject(pen);
                        }

                        BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
                        SelectObject(mem_dc, old_bmp);
                        DeleteObject(mem_bmp);
                        DeleteDC(mem_dc);
                    }
                }
            });
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn dialog_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            let ctrl = wparam as HWND;
            let id = GetDlgCtrlID(ctrl) as isize;
            if id == IDC_LINK_GITHUB || id == IDC_LINK_UPDATE || id == IDC_LINK_KOFI {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORSTATIC => {
            let ctrl = lparam as HWND;
            let hdc = wparam as _;
            SetBkMode(hdc, TRANSPARENT as i32);
            let id = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(ctrl) as isize;
            if id == IDC_LINK_GITHUB || id == IDC_LINK_UPDATE || id == IDC_LINK_KOFI {
                SetTextColor(hdc, 0x00CC6600 as COLORREF); // Blue link color
            } else {
                SetTextColor(hdc, 0x00222222 as COLORREF);
            }
            GetStockObject(windows_sys::Win32::Graphics::Gdi::WHITE_BRUSH) as LRESULT
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as isize;
            match id {
                IDC_BTN_COPY => {
                    let text = APP.with(|app| {
                        let st = app.borrow();
                        st.as_ref().unwrap().decoded_results.join("\n")
                    });
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(text);
                    }
                }
                IDC_BTN_OPEN_URL => {
                    let url = APP.with(|app| {
                        let st = app.borrow();
                        st.as_ref().unwrap().decoded_results.first().cloned()
                    });
                    if let Some(u) = url {
                        let _ = open::that(u);
                    }
                }
                IDC_BTN_RESCAN => {
                    ShowWindow(hwnd, SW_HIDE);
                    DestroyWindow(hwnd);
                    std::thread::sleep(Duration::from_millis(250));
                    start_capture();
                }
                IDC_BTN_CLOSE | IDC_BTN_UPDATE_LATER => {
                    DestroyWindow(hwnd);
                }
                IDC_LINK_GITHUB => {
                    let _ = open::that("https://github.com/zungmou/qr-scanner");
                }
                IDC_LINK_KOFI => {
                    let _ = open::that("https://ko-fi.com/zungmou");
                }
                IDC_LINK_UPDATE | IDC_BTN_UPDATE_GO => {
                    let url = APP.with(|app| {
                        let st = app.borrow();
                        st.as_ref()
                            .unwrap()
                            .update_info
                            .lock()
                            .ok()
                            .and_then(|g| g.clone())
                            .map(|i| i.url)
                    });
                    if let Some(u) = url {
                        let _ = open::that(u);
                    }
                    if id == IDC_BTN_UPDATE_GO {
                        DestroyWindow(hwnd);
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            APP.with(|app| {
                if let Some(ref mut st) = *app.borrow_mut() {
                    if st.result_hwnd == hwnd {
                        st.result_hwnd = null_mut();
                    } else if st.about_hwnd == hwnd {
                        st.about_hwnd = null_mut();
                    } else if st.update_hwnd == hwnd {
                        st.update_hwnd = null_mut();
                    }
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn register_classes(hinstance: HINSTANCE) {
    let icon = LoadIconW(hinstance, 1 as *const u16);

    let tray_cls = to_wide("QrScannerTrayClass");
    let wc_tray = WNDCLASSW {
        lpfnWndProc: Some(tray_wnd_proc),
        hInstance: hinstance,
        lpszClassName: tray_cls.as_ptr(),
        ..std::mem::zeroed()
    };
    RegisterClassW(&wc_tray);

    let ov_cls = to_wide("QrScannerOverlayClass");
    let wc_ov = WNDCLASSW {
        lpfnWndProc: Some(overlay_wnd_proc),
        hInstance: hinstance,
        hCursor: LoadCursorW(null_mut(), IDC_CROSS),
        lpszClassName: ov_cls.as_ptr(),
        ..std::mem::zeroed()
    };
    RegisterClassW(&wc_ov);

    let white_brush = GetStockObject(windows_sys::Win32::Graphics::Gdi::WHITE_BRUSH) as HBRUSH;
    let arrow = LoadCursorW(null_mut(), IDC_ARROW);

    for name in [
        "QrScannerResultClass",
        "QrScannerAboutClass",
        "QrScannerUpdateClass",
    ] {
        let cls = to_wide(name);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(dialog_wnd_proc),
            hInstance: hinstance,
            hIcon: icon,
            hCursor: arrow,
            hbrBackground: white_brush,
            lpszClassName: cls.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
    }
}

fn main() {
    unsafe {
        let hinstance = GetModuleHandleW(null());
        register_classes(hinstance);

        let tray_cls = to_wide("QrScannerTrayClass");
        let tray_title = to_wide("QR Scanner Tray");
        let tray_hwnd = CreateWindowExW(
            0,
            tray_cls.as_ptr(),
            tray_title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );

        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = tray_hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = LoadIconW(hinstance, 1 as *const u16);
        let initial_lang = load_lang();
        let tip = to_wide(match initial_lang {
            Lang::En => "QR Scanner (Click tray icon to scan)",
            Lang::Zh => "屏幕二维码扫描 (左键点击框选)",
        });
        let copy_len = tip.len().min(nid.szTip.len());
        nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
        Shell_NotifyIconW(NIM_ADD, &nid);

        let font = create_ui_font(false);
        let link_font = create_ui_font(true);
        let update_info = Arc::new(Mutex::new(None));

        APP.with(|app| {
            *app.borrow_mut() = Some(AppState {
                tray_hwnd,
                overlay_hwnd: null_mut(),
                result_hwnd: null_mut(),
                about_hwnd: null_mut(),
                update_hwnd: null_mut(),
                font,
                link_font,
                overlay: None,
                decoded_results: Vec::new(),
                update_info: Arc::clone(&update_info),
                lang: initial_lang,
            });
        });

        let tray_hwnd_bg = tray_hwnd as usize;
        std::thread::spawn(move || {
            if let Some(info) = check_github_update() {
                if let Ok(mut guard) = update_info.lock() {
                    *guard = Some(info);
                }
                PostMessageW(tray_hwnd_bg as HWND, WM_APP_UPDATE_FOUND, 0, 0);
            }
        });

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
