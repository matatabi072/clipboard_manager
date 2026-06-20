// クリップボード履歴＆定型文管理ツール
// Win32 API 直叩きによる軽量・常駐型ポータブルアプリ
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    GetLastError, GlobalFree, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateFontIndirectW, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetDC, GetDIBits, InvalidateRect,
    ReleaseDC, SelectObject, SetBkColor, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, COLOR_BTNFACE, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, HBITMAP, HBRUSH, HDC, HFONT,
    PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromHBITMAP, GdipCreateBitmapFromStream, GdipCreateHBITMAPFromBitmap,
    GdipDisposeImage, GdipGetImageHeight, GdipGetImageThumbnail, GdipGetImageWidth,
    GdipSaveImageToStream, GdiplusStartup, GdiplusStartupInput, GpBitmap, GpImage,
};
use windows_sys::Win32::System::Com::StructuredStorage::{
    CreateStreamOnHGlobal, GetHGlobalFromStream,
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, SetWindowTheme, ICC_HOTKEY_CLASS, ICC_STANDARD_CLASSES,
    INITCOMMONCONTROLSEX,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, RegisterHotKey, SendInput, SetFocus, UnregisterHotKey, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_CONTROL,
    VK_DELETE, VK_ESCAPE, VK_MENU, VK_RETURN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CF_UNICODETEXT: u32 = 13;
const CF_BITMAP: u32 = 2;
const CF_DIB: u32 = 8;
const EM_SETSEL: u32 = 0x00B1;
const HKM_SETHOTKEY: u32 = 0x0401;
const HKM_GETHOTKEY: u32 = 0x0402;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0;
const OBJID_WINDOW: i32 = 0;

// ダークモード配色 (文字は背景に同化させない: 明るい文字×暗い背景)
const DARK_BG_WINDOW: u32 = 0x00202020;
const DARK_BG_CTRL: u32 = 0x002B2B2B;
const DARK_TEXT: u32 = 0x00F0F0F0;

// コントロールID
const ID_MODE: i32 = 100;
const ID_LIST: i32 = 102;
const ID_PASTE: i32 = 110;
const ID_TO_SNIP: i32 = 111;
const ID_ADD: i32 = 112;
const ID_EDIT: i32 = 113;
const ID_DELETE: i32 = 114;
const ID_SETTINGS: i32 = 115;
const ID_IMAGES: i32 = 116; // 画像モードの「全消去」ボタン
// タイマー
const ID_TIMER_CLIP: usize = 1; // クリップボードポーリング(500ms)
// トレイメニュー
const ID_TRAY_SHOW: i32 = 200;
const ID_TRAY_EXIT: i32 = 201;
// ダイアログ
const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_HK_ASSIGN: i32 = 3;
const ID_HK_CLEAR: i32 = 4;
const ID_HK_LIST: i32 = 5;

const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_SELF: u32 = WM_APP + 2;

// ホットキーアクション (インデックス → RegisterHotKey ID = 100+i)
// 0:表示  1:定型文登録  2-10:履歴1-9貼り付け  11-19:履歴1-9消去  20-28:定型文1-9貼り付け
// 29:常に手前に表示 切り替え
const HK_COUNT: usize = 30;
const HK_ID_BASE: i32 = 100;

// 画像保持の上限とサムネイルサイズ
const IMAGE_MAX: usize = 10;
const THUMB_MAX: u32 = 84;

// PNGエンコーダのCLSID {557CF406-1A04-11D3-9A73-0000F81EF32E}
const PNG_ENCODER_CLSID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x557CF406,
    data2: 0x1A04,
    data3: 0x11D3,
    data4: [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E],
};

// メモリ保持する画像 (終了・全消去でメモリごと破棄)
struct ImageItem {
    png: Vec<u8>,   // PNG圧縮データ (貼り付け・サムネイル再生成用)
    thumb: HBITMAP, // 一覧描画用サムネイル (DDB)
    tw: i32,        // サムネイル幅
    th: i32,        // サムネイル高さ
}

impl Drop for ImageItem {
    fn drop(&mut self) {
        if !self.thumb.is_null() {
            unsafe { DeleteObject(self.thumb as _) };
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    History,
    Snippet,
    Image,
}

struct App {
    list: HWND,
    lbl_mode: HWND,
    btn_mode: HWND,
    btn_paste: HWND,
    btn_to_snip: HWND,
    btn_add: HWND,
    btn_edit: HWND,
    btn_delete: HWND,
    btn_settings: HWND,
    btn_images: HWND,
    font: HFONT,
    list_font: HFONT,
    dark: bool,
    br_window: HBRUSH,
    br_ctrl: HBRUSH,
    tab: Tab,
    history: Vec<String>,
    snippets: Vec<String>,
    images: Vec<ImageItem>, // クリップボード画像履歴 (メモリ保持・非永続)
    history_max: usize,
    snippet_max: usize,
    font_size: u32,
    topmost: bool,
    move_to_top: bool, // 再コピー時に既存項目を履歴の先頭へ移動
    hotkeys: [(u32, u32); HK_COUNT], // (修飾キー, 仮想キー) vk=0で無効
    suppress: bool, // 自分でクリップボードに書いた直後の通知を無視
    target: HWND,   // 貼り付け先(呼び出し時のフォアグラウンドウィンドウ)
    data_path: PathBuf,
    cf_png: u32,         // 登録クリップボード形式 "PNG"
    cf_imgpng: u32,     // Chrome/Edge の "image/png" クリップボード形式
    clip_seq: u32,      // クリップボードシーケンス番号(ポーリング用)
    img_panel: HWND,     // 画像モードのサムネイル描画パネル(メインの子)
    img_selected: isize, // 画像の選択インデックス
    // 画像モードに入る直前のウィンドウ矩形(位置+サイズ)。戻すときに復元する
    saved_rect: Option<(i32, i32, i32, i32)>, // (x, y, w, h)
}

fn default_hotkeys() -> [(u32, u32); HK_COUNT] {
    let mut hk = [(0u32, 0u32); HK_COUNT];
    hk[1] = (MOD_CONTROL | MOD_ALT, 0x56); // 定型文に登録: Ctrl+Alt+V
    for i in 0..9 {
        // 履歴1〜9貼り付け: Ctrl+Alt+1〜9
        // (Ctrl+Shift+数字はExcelのセル書式ショートカットと衝突するため不採用)
        hk[2 + i] = (MOD_CONTROL | MOD_ALT, 0x31 + i as u32);
    }
    hk
}

impl Default for App {
    fn default() -> Self {
        App {
            list: null_mut(),
            lbl_mode: null_mut(),
            btn_mode: null_mut(),
            btn_paste: null_mut(),
            btn_to_snip: null_mut(),
            btn_add: null_mut(),
            btn_edit: null_mut(),
            btn_delete: null_mut(),
            btn_settings: null_mut(),
            btn_images: null_mut(),
            font: null_mut(),
            list_font: null_mut(),
            dark: false,
            br_window: null_mut(),
            br_ctrl: null_mut(),
            tab: Tab::History,
            history: Vec::new(),
            snippets: Vec::new(),
            images: Vec::new(),
            history_max: 10,
            snippet_max: 10,
            font_size: 10,
            topmost: true,
            move_to_top: true,
            hotkeys: default_hotkeys(),
            suppress: false,
            target: null_mut(),
            data_path: PathBuf::new(),
            cf_png: 0,
            cf_imgpng: 0,
            clip_seq: 0,
            img_panel: null_mut(),
            img_selected: -1,
            saved_rect: None,
        }
    }
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

fn wz(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ============ ホットキー名称 ============

fn action_label(i: usize) -> String {
    match i {
        0 => "ウィンドウ表示/非表示".to_string(),
        1 => "クリップボードを定型文に登録".to_string(),
        2..=10 => format!("履歴{} 貼り付け", i - 1),
        11..=19 => format!("履歴{} 消去", i - 10),
        20..=28 => format!("定型文{} 貼り付け", i - 19),
        _ => "常に手前に表示 切り替え".to_string(),
    }
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).unwrap().to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x08 => "BackSpace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        _ => format!("0x{:02X}", vk),
    }
}

fn key_name(m: u32, vk: u32) -> String {
    if vk == 0 {
        return "(なし)".to_string();
    }
    let mut s = String::new();
    if m & MOD_CONTROL != 0 {
        s.push_str("Ctrl+");
    }
    if m & MOD_SHIFT != 0 {
        s.push_str("Shift+");
    }
    if m & MOD_ALT != 0 {
        s.push_str("Alt+");
    }
    s.push_str(&vk_name(vk));
    s
}

// HOTKEYコントロールの修飾フラグ(HOTKEYF_*) ⇔ RegisterHotKeyの修飾フラグ(MOD_*)
fn hotkeyf_to_mod(hf: u32) -> u32 {
    let mut m = 0;
    if hf & 1 != 0 {
        m |= MOD_SHIFT;
    }
    if hf & 2 != 0 {
        m |= MOD_CONTROL;
    }
    if hf & 4 != 0 {
        m |= MOD_ALT;
    }
    m
}

fn mod_to_hotkeyf(m: u32) -> u32 {
    let mut h = 0;
    if m & MOD_SHIFT != 0 {
        h |= 1;
    }
    if m & MOD_CONTROL != 0 {
        h |= 2;
    }
    if m & MOD_ALT != 0 {
        h |= 4;
    }
    h
}

// ============ 永続化 ============

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\r', "\\r").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(x) => out.push(x),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn save_state() {
    let (path, data) = with_app(|a| {
        let mut s = String::new();
        s.push_str(&format!("history_max={}\n", a.history_max));
        s.push_str(&format!("snippet_max={}\n", a.snippet_max));
        s.push_str(&format!("font_size={}\n", a.font_size));
        s.push_str(&format!("topmost={}\n", if a.topmost { 1 } else { 0 }));
        s.push_str(&format!("move_to_top={}\n", if a.move_to_top { 1 } else { 0 }));
        for (i, (m, vk)) in a.hotkeys.iter().enumerate() {
            s.push_str(&format!("hk{}={},{}\n", i, m, vk));
        }
        s.push_str("[history]\n");
        for h in &a.history {
            s.push_str(&escape(h));
            s.push('\n');
        }
        s.push_str("[snippets]\n");
        for sn in &a.snippets {
            s.push_str(&escape(sn));
            s.push('\n');
        }
        (a.data_path.clone(), s)
    });
    let _ = std::fs::write(&path, data);
}

fn load_state() {
    let path = std::env::current_exe()
        .map(|p| p.with_file_name("clipboard_manager_data.txt"))
        .unwrap_or_else(|_| PathBuf::from("clipboard_manager_data.txt"));
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    with_app(|a| {
        a.data_path = path;
        let mut section = "";
        for line in content.lines() {
            if line == "[history]" {
                section = "history";
            } else if line == "[snippets]" {
                section = "snippets";
            } else if section == "history" {
                a.history.push(unescape(line));
            } else if section == "snippets" {
                a.snippets.push(unescape(line));
            } else if let Some(v) = line.strip_prefix("history_max=") {
                a.history_max = v.trim().parse().unwrap_or(10).clamp(1, 1000);
            } else if let Some(v) = line.strip_prefix("snippet_max=") {
                a.snippet_max = v.trim().parse().unwrap_or(10).clamp(1, 1000);
            } else if let Some(v) = line.strip_prefix("font_size=") {
                a.font_size = v.trim().parse().unwrap_or(10).clamp(6, 40);
            } else if let Some(v) = line.strip_prefix("topmost=") {
                a.topmost = v.trim() != "0";
            } else if let Some(v) = line.strip_prefix("move_to_top=") {
                a.move_to_top = v.trim() != "0";
            } else if let Some(rest) = line.strip_prefix("hk") {
                if let Some((idx, val)) = rest.split_once('=') {
                    if let (Ok(i), Some((m, vk))) = (idx.parse::<usize>(), val.split_once(',')) {
                        if i < HK_COUNT {
                            a.hotkeys[i] = (
                                m.trim().parse().unwrap_or(0),
                                vk.trim().parse().unwrap_or(0),
                            );
                        }
                    }
                }
            }
        }
        let max = a.history_max;
        a.history.truncate(max);
    });
}

// ============ クリップボード ============

unsafe fn open_clipboard_retry(hwnd: HWND) -> bool {
    for _ in 0..5 {
        if OpenClipboard(hwnd) != 0 {
            return true;
        }
        sleep(Duration::from_millis(10));
    }
    false
}

unsafe fn get_clipboard_text(hwnd: HWND) -> Option<String> {
    if !open_clipboard_retry(hwnd) {
        return None;
    }
    let mut result = None;
    let h = GetClipboardData(CF_UNICODETEXT);
    if !h.is_null() {
        let p = GlobalLock(h) as *const u16;
        if !p.is_null() {
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            result = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
            GlobalUnlock(h);
        }
    }
    CloseClipboard();
    result
}

unsafe fn set_clipboard_text(hwnd: HWND, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if !open_clipboard_retry(hwnd) {
        return false;
    }
    EmptyClipboard();
    let h = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
    let mut ok = false;
    if !h.is_null() {
        let p = GlobalLock(h) as *mut u16;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(units.as_ptr(), p, units.len());
            GlobalUnlock(h);
            if SetClipboardData(CF_UNICODETEXT, h).is_null() {
                GlobalFree(h); // SetClipboardData 失敗時は所有権が戻るので解放
            } else {
                ok = true;
            }
        } else {
            GlobalFree(h); // GlobalLock 失敗時も解放
        }
    }
    CloseClipboard();
    ok
}

unsafe fn send_ctrl_v() {
    fn key(vk: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
            },
        }
    }
    // ホットキー由来の修飾キーが押されたままでも正しくCtrl+Vになるよう先に解放
    let inputs = [
        key(VK_SHIFT, KEYEVENTF_KEYUP),
        key(VK_MENU, KEYEVENTF_KEYUP),
        key(VK_CONTROL, 0),
        key(0x56, 0), // 'V'
        key(0x56, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
}

// ============ 画像 (GDI+) ============

// IStream の Release を手動 vtable 経由で呼ぶ (COMの最小利用)
unsafe fn istream_release(stream: *mut c_void) {
    if stream.is_null() {
        return;
    }
    let vtbl = *(stream as *const *const usize);
    let release: unsafe extern "system" fn(*mut c_void) -> u32 =
        std::mem::transmute(*vtbl.add(2));
    release(stream);
}

fn scaled_size(w: u32, h: u32, maxside: u32) -> (u32, u32) {
    if w <= maxside && h <= maxside {
        return (w.max(1), h.max(1));
    }
    if w >= h {
        (maxside, (h * maxside / w).max(1))
    } else {
        ((w * maxside / h).max(1), maxside)
    }
}

unsafe fn bytes_to_hglobal(data: &[u8]) -> *mut c_void {
    let hg = GlobalAlloc(GMEM_MOVEABLE, data.len());
    if hg.is_null() {
        return null_mut();
    }
    let p = GlobalLock(hg) as *mut u8;
    if p.is_null() {
        GlobalFree(hg);
        return null_mut();
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
    GlobalUnlock(hg);
    hg
}

// GpImage を PNG バイト列にエンコード
unsafe fn encode_png(img: *mut GpImage) -> Option<Vec<u8>> {
    let mut stream: *mut c_void = null_mut();
    if CreateStreamOnHGlobal(null_mut(), 1, &mut stream) != 0 || stream.is_null() {
        return None;
    }
    let mut result = None;
    if GdipSaveImageToStream(img, stream, &PNG_ENCODER_CLSID, null()) == 0 {
        let mut hg: *mut c_void = null_mut();
        if GetHGlobalFromStream(stream, &mut hg) == 0 && !hg.is_null() {
            let size = GlobalSize(hg);
            let p = GlobalLock(hg) as *const u8;
            if !p.is_null() && size > 0 {
                result = Some(std::slice::from_raw_parts(p, size).to_vec());
                GlobalUnlock(hg);
            }
        }
    }
    istream_release(stream); // fDeleteOnRelease=TRUE なので HGLOBAL も解放される
    result
}

// PNG バイト列から GpBitmap を生成
unsafe fn decode_png(png: &[u8]) -> *mut GpBitmap {
    let hg = bytes_to_hglobal(png);
    if hg.is_null() {
        return null_mut();
    }
    let mut stream: *mut c_void = null_mut();
    if CreateStreamOnHGlobal(hg, 1, &mut stream) != 0 || stream.is_null() {
        GlobalFree(hg);
        return null_mut();
    }
    let mut gpbmp: *mut GpBitmap = null_mut();
    GdipCreateBitmapFromStream(stream, &mut gpbmp);
    istream_release(stream); // HGLOBAL も解放
    gpbmp
}

// GpImage から一覧表示用サムネイル(DDB)を生成
unsafe fn make_thumbnail(img: *mut GpImage) -> Option<(HBITMAP, i32, i32)> {
    let mut w = 0u32;
    let mut h = 0u32;
    GdipGetImageWidth(img, &mut w);
    GdipGetImageHeight(img, &mut h);
    if w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = scaled_size(w, h, THUMB_MAX);
    let mut thumb_img: *mut GpImage = null_mut();
    if GdipGetImageThumbnail(img, tw, th, &mut thumb_img, 0, null_mut()) != 0 || thumb_img.is_null()
    {
        return None;
    }
    let mut hbm: HBITMAP = null_mut();
    let st = GdipCreateHBITMAPFromBitmap(thumb_img as *mut GpBitmap, &mut hbm, 0x00FFFFFF);
    GdipDisposeImage(thumb_img);
    if st != 0 || hbm.is_null() {
        return None;
    }
    Some((hbm, tw as i32, th as i32))
}

// PNG バイト列 → GpBitmap (decode_png 経由) → ImageItem
// Chrome/Edge の "image/png" クリップボードデータはそのまま PNG なのでサムネイルだけ作る
unsafe fn png_bytes_to_item(png: Vec<u8>) -> Option<ImageItem> {
    let gpbmp = decode_png(&png);
    if gpbmp.is_null() {
        return None;
    }
    let img = gpbmp as *mut GpImage;
    let result = if let Some((thumb, tw, th)) = make_thumbnail(img) {
        Some(ImageItem { png, thumb, tw, th })
    } else {
        None
    };
    GdipDisposeImage(img);
    result
}

// クリップボードの画像を取り込み、ImageItem を生成
unsafe fn capture_clipboard_image(hwnd: HWND) -> Option<ImageItem> {
    let cf_imgpng = with_app(|a| a.cf_imgpng);
    let has_bitmap = IsClipboardFormatAvailable(CF_BITMAP) != 0
        || IsClipboardFormatAvailable(CF_DIB) != 0;
    let has_imgpng = cf_imgpng != 0 && IsClipboardFormatAvailable(cf_imgpng) != 0;

    if !has_bitmap && !has_imgpng {
        return None;
    }
    if !open_clipboard_retry(hwnd) {
        return None;
    }

    // "image/png" を優先（Chrome/Edge はこちらだけ置くことが多い）
    if has_imgpng {
        let hg = GetClipboardData(cf_imgpng);
        if !hg.is_null() {
            let p = GlobalLock(hg) as *const u8;
            if !p.is_null() {
                let size = GlobalSize(hg);
                let png = std::slice::from_raw_parts(p, size).to_vec();
                GlobalUnlock(hg);
                CloseClipboard();
                return png_bytes_to_item(png);
            }
        }
    }

    // フォールバック: CF_BITMAP / CF_DIB
    let hbm = GetClipboardData(CF_BITMAP); // CF_DIB からも自動合成される
    let mut item = None;
    if !hbm.is_null() {
        let mut gpbmp: *mut GpBitmap = null_mut();
        if GdipCreateBitmapFromHBITMAP(hbm as HBITMAP, null_mut(), &mut gpbmp) == 0
            && !gpbmp.is_null()
        {
            let img = gpbmp as *mut GpImage;
            if let (Some(png), Some((thumb, tw, th))) = (encode_png(img), make_thumbnail(img)) {
                item = Some(ImageItem { png, thumb, tw, th });
            }
            GdipDisposeImage(img);
        }
    }
    CloseClipboard();
    item
}

// HBITMAP を CF_DIB 形式の HGLOBAL に変換
unsafe fn hbitmap_to_dib(hbm: HBITMAP) -> *mut c_void {
    if hbm.is_null() {
        return null_mut();
    }
    let screen = GetDC(null_mut());
    let mut bi: BITMAPINFO = zeroed();
    bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    if GetDIBits(screen, hbm, 0, 0, null_mut(), &mut bi, DIB_RGB_COLORS) == 0 {
        ReleaseDC(null_mut(), screen);
        return null_mut();
    }
    let width = bi.bmiHeader.biWidth;
    let height = bi.bmiHeader.biHeight.abs();
    bi.bmiHeader.biHeight = height; // bottom-up DIB
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB as u32;
    let img_size = (width * height * 4) as usize;
    bi.bmiHeader.biSizeImage = img_size as u32;
    let header = size_of::<BITMAPINFOHEADER>();
    let hg = GlobalAlloc(GMEM_MOVEABLE, header + img_size);
    if hg.is_null() {
        ReleaseDC(null_mut(), screen);
        return null_mut();
    }
    let p = GlobalLock(hg) as *mut u8;
    if p.is_null() {
        GlobalFree(hg);
        ReleaseDC(null_mut(), screen);
        return null_mut();
    }
    std::ptr::copy_nonoverlapping(&bi.bmiHeader as *const _ as *const u8, p, header);
    GetDIBits(
        screen,
        hbm,
        0,
        height as u32,
        p.add(header) as *mut c_void,
        &mut bi,
        DIB_RGB_COLORS,
    );
    GlobalUnlock(hg);
    ReleaseDC(null_mut(), screen);
    hg
}

// 画像をクリップボードへ複数形式(CF_BITMAP/CF_DIB/"PNG")で置いて貼り付け
unsafe fn paste_image(hwnd: HWND, index: usize) {
    let (png, target, cf_png) =
        with_app(|a| (a.images.get(index).map(|i| i.png.clone()), a.target, a.cf_png));
    let Some(png) = png else { return };

    let gpbmp = decode_png(&png);
    if gpbmp.is_null() {
        return;
    }
    let mut hbm: HBITMAP = null_mut();
    GdipCreateHBITMAPFromBitmap(gpbmp, &mut hbm, 0x00FFFFFF);
    GdipDisposeImage(gpbmp as *mut GpImage);
    let hdib = hbitmap_to_dib(hbm);
    let hpng = bytes_to_hglobal(&png);

    let ok = open_clipboard_retry(hwnd);
    with_app(|a| a.suppress = ok);
    if !ok {
        if !hbm.is_null() {
            DeleteObject(hbm as _);
        }
        if !hdib.is_null() {
            GlobalFree(hdib);
        }
        if !hpng.is_null() {
            GlobalFree(hpng);
        }
        return;
    }
    EmptyClipboard();
    // SetClipboardData 成功時、ハンドルの所有はシステムへ移る
    if !hbm.is_null() {
        SetClipboardData(CF_BITMAP, hbm as _);
    }
    if !hdib.is_null() {
        SetClipboardData(CF_DIB, hdib as _);
    }
    if cf_png != 0 && !hpng.is_null() {
        SetClipboardData(cf_png, hpng as _);
    }
    CloseClipboard();

    if !target.is_null() && IsWindow(target) != 0 {
        SetForegroundWindow(target);
    }
    sleep(Duration::from_millis(80));
    send_ctrl_v();
}

// ============ ダークモード ============

unsafe fn detect_dark() -> bool {
    let mut val: u32 = 1;
    let mut size: u32 = 4;
    RegGetValueW(
        HKEY_CURRENT_USER,
        wz("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
        wz("AppsUseLightTheme").as_ptr(),
        RRF_RT_REG_DWORD,
        null_mut(),
        &mut val as *mut _ as *mut c_void,
        &mut size,
    );
    val == 0
}

unsafe fn dwm_dark(hwnd: HWND, dark: bool) {
    let v: i32 = if dark { 1 } else { 0 };
    DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as *const c_void, 4);
}

unsafe fn theme_control(h: HWND, dark: bool) {
    let theme = if dark { wz("DarkMode_Explorer") } else { wz("Explorer") };
    SetWindowTheme(h, theme.as_ptr(), null());
}

unsafe fn apply_theme(hwnd: HWND) {
    let dark = detect_dark();
    let controls = with_app(|a| {
        a.dark = dark;
        if !a.br_window.is_null() {
            DeleteObject(a.br_window as _);
            DeleteObject(a.br_ctrl as _);
            a.br_window = null_mut();
            a.br_ctrl = null_mut();
        }
        if dark {
            a.br_window = CreateSolidBrush(DARK_BG_WINDOW);
            a.br_ctrl = CreateSolidBrush(DARK_BG_CTRL);
        }
        vec![
            a.list, a.lbl_mode, a.btn_mode, a.btn_paste, a.btn_to_snip, a.btn_add, a.btn_edit,
            a.btn_delete, a.btn_settings, a.btn_images, a.img_panel,
        ]
    });
    dwm_dark(hwnd, dark);
    for c in &controls {
        theme_control(*c, dark);
        InvalidateRect(*c, null(), 1);
    }
    InvalidateRect(hwnd, null(), 1);
}

// WM_CTLCOLOR系の共通処理。ダーク時のみ独自配色 (明るい文字×暗い背景で同化を防ぐ)
unsafe fn ctl_color(msg: u32, hdc: HDC) -> Option<LRESULT> {
    let (dark, br_window, br_ctrl) = with_app(|a| (a.dark, a.br_window, a.br_ctrl));
    if !dark {
        return None;
    }
    SetTextColor(hdc, DARK_TEXT);
    match msg {
        WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT => {
            SetBkColor(hdc, DARK_BG_CTRL);
            Some(br_ctrl as LRESULT)
        }
        _ => {
            SetBkColor(hdc, DARK_BG_WINDOW);
            Some(br_window as LRESULT)
        }
    }
}

unsafe fn erase_bkgnd(hwnd: HWND, hdc: HDC) -> Option<LRESULT> {
    let (dark, br) = with_app(|a| (a.dark, a.br_window));
    if !dark || br.is_null() {
        return None;
    }
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);
    FillRect(hdc, &rc, br);
    Some(1)
}

// ============ UI ヘルパー ============

fn display_string(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let trimmed = one_line.trim();
    let mut out: String = trimmed.chars().take(80).collect();
    if trimmed.chars().count() > 80 {
        out.push('…');
    }
    out
}

unsafe fn refresh_list(keep_sel: bool) {
    let (list, items) = with_app(|a| {
        let src = match a.tab {
            Tab::History => &a.history,
            Tab::Snippet => &a.snippets,
            Tab::Image => return (a.list, Vec::new()), // 画像モードはリスト未使用
        };
        let items: Vec<String> = src
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}: {}", i + 1, display_string(s)))
            .collect();
        (a.list, items)
    });
    let prev = SendMessageW(list, LB_GETCURSEL, 0, 0);
    SendMessageW(list, WM_SETREDRAW, 0, 0);
    SendMessageW(list, LB_RESETCONTENT, 0, 0);
    for it in &items {
        let w = wz(it);
        SendMessageW(list, LB_ADDSTRING, 0, w.as_ptr() as LPARAM);
    }
    let count = items.len() as isize;
    if count > 0 {
        let sel = if keep_sel && prev >= 0 { prev.min(count - 1) } else { 0 };
        SendMessageW(list, LB_SETCURSEL, sel as WPARAM, 0);
    }
    SendMessageW(list, WM_SETREDRAW, 1, 0);
    InvalidateRect(list, null(), 1);
}

// 画像モードでは必要に応じてウィンドウ幅を広げ(画面外に出ない)、
// 戻すときは画像モードに入る前の位置・サイズに復元する
unsafe fn adjust_window_for_mode(hwnd: HWND, tab: Tab) {
    if tab == Tab::Image {
        if with_app(|a| a.saved_rect.is_some()) {
            return; // 既に画像モード用に調整済み
        }
        let mut wr: RECT = zeroed();
        GetWindowRect(hwnd, &mut wr);
        let (x, y, cw, ch) = (wr.left, wr.top, wr.right - wr.left, wr.bottom - wr.top);
        with_app(|a| a.saved_rect = Some((x, y, cw, ch)));

        // 画像3列ぶんに必要なウィンドウ全体幅を算出
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let mut need = RECT { left: 0, top: 0, right: IMG_PANEL_W, bottom: 100 };
        AdjustWindowRect(&mut need, style, 0);
        let mut work: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as *mut c_void, 0);
        // 既により広ければ維持。画面幅は超えないようクランプ
        let want_w = (need.right - need.left).max(cw).min(work.right - work.left);
        // 右にはみ出すなら左へずらし、左端でクランプ (画面外に出さない)
        let mut nx = x;
        if nx + want_w > work.right {
            nx = work.right - want_w;
        }
        if nx < work.left {
            nx = work.left;
        }
        SetWindowPos(hwnd, null_mut(), nx, y, want_w, ch, SWP_NOZORDER);
    } else if let Some((x, y, cw, ch)) = with_app(|a| a.saved_rect.take()) {
        // 画像モードから戻る: 元の位置・サイズを復元
        SetWindowPos(hwnd, null_mut(), x, y, cw, ch, SWP_NOZORDER);
    }
}

unsafe fn update_tab_ui(hwnd: HWND) {
    let (tab, btn) = with_app(|a| (a.tab, a.btn_mode));
    let label = match tab {
        Tab::History => "履歴",
        Tab::Snippet => "定型文",
        Tab::Image => "画像",
    };
    SetWindowTextW(btn, wz(label).as_ptr());
    adjust_window_for_mode(hwnd, tab);
    layout(hwnd);
    if tab == Tab::Image {
        with_app(|a| {
            if a.img_selected < 0 && !a.images.is_empty() {
                a.img_selected = 0;
            }
        });
    } else {
        refresh_list(false);
    }
}

unsafe fn layout(hwnd: HWND) {
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let (lbl, mode, list, panel, paste, to_snip, add, edit, del, settings, images, tab) =
        with_app(|a| {
            (
                a.lbl_mode, a.btn_mode, a.list, a.img_panel, a.btn_paste, a.btn_to_snip,
                a.btn_add, a.btn_edit, a.btn_delete, a.btn_settings, a.btn_images, a.tab,
            )
        });

    // 上段: 「モード」ラベル + トグルボタン
    MoveWindow(lbl, 6, 7, 54, 20, 1);
    MoveWindow(mode, 62, 3, w - 68, 26, 1);
    // 中央: リスト/画像パネルは同じ領域を共有
    let mid_h = h - 32 - 72;
    MoveWindow(list, 0, 32, w, mid_h, 1);
    MoveWindow(panel, 0, 32, w, mid_h, 1);

    // 下段2列のボタン配置
    let row1 = h - 68;
    let row2 = h - 34;
    let bh = 28;
    let half = (w - 18) / 2;
    let third = (w - 24) / 3;
    match tab {
        Tab::History => {
            // 中央: リスト表示 / パネル非表示
            ShowWindow(list, SW_SHOW);
            ShowWindow(panel, SW_HIDE);
            // 上段: 貼り付け / 定型文に登録
            MoveWindow(paste, 6, row1, half, bh, 1);
            MoveWindow(to_snip, 12 + half, row1, half, bh, 1);
            ShowWindow(paste, SW_SHOW);
            ShowWindow(to_snip, SW_SHOW);
            ShowWindow(add, SW_HIDE);
            ShowWindow(edit, SW_HIDE);
            ShowWindow(images, SW_HIDE);
            // 下段: 削除 / 設定
            MoveWindow(del, 6, row2, half, bh, 1);
            MoveWindow(settings, 12 + half, row2, half, bh, 1);
            ShowWindow(del, SW_SHOW);
            ShowWindow(settings, SW_SHOW);
        }
        Tab::Snippet => {
            ShowWindow(list, SW_SHOW);
            ShowWindow(panel, SW_HIDE);
            MoveWindow(paste, 6, row1, half, bh, 1);
            MoveWindow(add, 12 + half, row1, half, bh, 1);
            ShowWindow(paste, SW_SHOW);
            ShowWindow(to_snip, SW_HIDE);
            ShowWindow(add, SW_SHOW);
            ShowWindow(edit, SW_SHOW);
            ShowWindow(images, SW_HIDE);
            MoveWindow(edit, 6, row2, third, bh, 1);
            MoveWindow(del, 12 + third, row2, third, bh, 1);
            MoveWindow(settings, 18 + third * 2, row2, third, bh, 1);
            ShowWindow(del, SW_SHOW);
            ShowWindow(settings, SW_SHOW);
        }
        Tab::Image => {
            // 中央: パネル表示 / リスト非表示
            ShowWindow(list, SW_HIDE);
            ShowWindow(panel, SW_SHOW);
            InvalidateRect(panel, null(), 1);
            // 上段: 貼り付け / 全消去  下段: 削除 / 設定
            MoveWindow(paste, 6, row1, half, bh, 1);
            MoveWindow(images, 12 + half, row1, half, bh, 1);
            ShowWindow(paste, SW_SHOW);
            ShowWindow(images, SW_SHOW);
            ShowWindow(to_snip, SW_HIDE);
            ShowWindow(add, SW_HIDE);
            ShowWindow(edit, SW_HIDE);
            MoveWindow(del, 6, row2, half, bh, 1);
            MoveWindow(settings, 12 + half, row2, half, bh, 1);
            ShowWindow(del, SW_SHOW);
            ShowWindow(settings, SW_SHOW);
        }
    }
}

// フォアグラウンドウィンドウの変化を常時追跡し、自プロセス以外を貼り付け先として記憶。
// (ウィンドウを開いたまま他アプリで作業しても、常に直前の作業ウィンドウへ貼り付けられる)
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    _idchild: i32,
    _tid: u32,
    _time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND || idobject != OBJID_WINDOW || hwnd.is_null() {
        return;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != GetCurrentProcessId() {
        with_app(|a| a.target = hwnd);
    }
}

unsafe fn apply_topmost(hwnd: HWND, show: bool) {
    let topmost = with_app(|a| a.topmost);
    let z = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let flags = SWP_NOSIZE | SWP_NOMOVE | if show { SWP_SHOWWINDOW } else { 0 };
    SetWindowPos(hwnd, z, 0, 0, 0, 0, flags);
}

unsafe fn show_main(hwnd: HWND) {
    // 表示位置は変更しない(ユーザーがドラッグした位置を維持)
    if IsIconic(hwnd) != 0 {
        ShowWindow(hwnd, SW_RESTORE);
    }
    apply_topmost(hwnd, true);
    SetForegroundWindow(hwnd);
    refresh_list(false);
    let list = with_app(|a| a.list);
    SetFocus(list);
}

unsafe fn selected_index() -> Option<usize> {
    let list = with_app(|a| a.list);
    let sel = SendMessageW(list, LB_GETCURSEL, 0, 0);
    if sel < 0 {
        None
    } else {
        Some(sel as usize)
    }
}

// ウィンドウ操作からの貼り付け (ウィンドウは閉じず、貼り付け先へフォーカスを戻す)
unsafe fn do_paste(hwnd: HWND) {
    let Some(sel) = selected_index() else { return };
    let (text, target) = with_app(|a| {
        let items = match a.tab {
            Tab::History => &a.history,
            Tab::Snippet => &a.snippets,
            Tab::Image => return (None, a.target), // ID_PASTEで先に処理済み
        };
        (items.get(sel).cloned(), a.target)
    });
    let Some(text) = text else { return };
    // 成功時のみ「自分の書き込み」を抑制 (失敗時にフラグが残ると次の正規コピーを取りこぼす)
    let ok = set_clipboard_text(hwnd, &text);
    with_app(|a| a.suppress = ok);
    if !ok {
        return;
    }
    if !target.is_null() && IsWindow(target) != 0 {
        SetForegroundWindow(target);
    }
    sleep(Duration::from_millis(80));
    send_ctrl_v();
}

// グローバルホットキーからの直接貼り付け (フォアグラウンドはそのまま)
unsafe fn direct_paste(hwnd: HWND, text: String) {
    // 成功時のみ抑制フラグを立てる (失敗時に残すと次の正規コピーを取りこぼす)
    if set_clipboard_text(hwnd, &text) {
        with_app(|a| a.suppress = true);
        sleep(Duration::from_millis(30));
        send_ctrl_v();
    }
}

unsafe fn message_box(hwnd: HWND, text: &str, title: &str) {
    MessageBoxW(hwnd, wz(text).as_ptr(), wz(title).as_ptr(), MB_OK | MB_ICONINFORMATION);
}

// ============ ホットキー登録 ============

unsafe fn apply_hotkeys(hwnd: HWND) -> Vec<usize> {
    let hks = with_app(|a| a.hotkeys);
    let mut failed = Vec::new();
    for (i, (m, vk)) in hks.iter().enumerate() {
        UnregisterHotKey(hwnd, HK_ID_BASE + i as i32);
        if *vk != 0 && RegisterHotKey(hwnd, HK_ID_BASE + i as i32, *m, *vk) == 0 {
            failed.push(i);
        }
    }
    failed
}

unsafe fn handle_hotkey(hwnd: HWND, idx: usize) {
    match idx {
        0 => {
            if IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0 {
                ShowWindow(hwnd, SW_HIDE);
            } else {
                show_main(hwnd);
            }
        }
        1 => {
            // 現在のクリップボード内容を定型文に登録
            if let Some(text) = get_clipboard_text(hwnd) {
                if !text.is_empty() {
                    let added = with_app(|a| {
                        if a.snippets.len() < a.snippet_max {
                            a.snippets.push(text);
                            true
                        } else {
                            false
                        }
                    });
                    if added {
                        save_state();
                        if with_app(|a| a.tab == Tab::Snippet) {
                            refresh_list(true);
                        }
                    }
                }
            }
        }
        2..=10 => {
            let n = idx - 2;
            if let Some(text) = with_app(|a| a.history.get(n).cloned()) {
                direct_paste(hwnd, text);
            }
        }
        11..=19 => {
            let n = idx - 11;
            let removed = with_app(|a| {
                if n < a.history.len() {
                    a.history.remove(n);
                    true
                } else {
                    false
                }
            });
            if removed {
                save_state();
                if with_app(|a| a.tab == Tab::History) {
                    refresh_list(true);
                }
            }
        }
        20..=28 => {
            let n = idx - 20;
            if let Some(text) = with_app(|a| a.snippets.get(n).cloned()) {
                direct_paste(hwnd, text);
            }
        }
        29 => {
            with_app(|a| a.topmost = !a.topmost);
            apply_topmost(hwnd, false);
            save_state();
        }
        _ => {}
    }
}

// ============ モーダルダイアログ ============

struct DlgState {
    done: bool,
    ok: bool,
    edits: Vec<HWND>,
    results: Vec<String>,
    hk_list: HWND,
    hk_ctrl: HWND,
    hk_vals: Vec<(u32, u32)>,
    checks: Vec<HWND>,
    check_vals: Vec<bool>,
}

unsafe fn read_window_text(h: HWND) -> String {
    let len = GetWindowTextLengthW(h);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let got = GetWindowTextW(h, buf.as_mut_ptr(), len + 1);
    String::from_utf16_lossy(&buf[..got.max(0) as usize])
}

unsafe fn hk_list_label(i: usize, m: u32, vk: u32) -> Vec<u16> {
    wz(&format!("{}\t{}", action_label(i), key_name(m, vk)))
}

unsafe fn hk_update_row(state: &mut DlgState, sel: usize) {
    let (m, vk) = state.hk_vals[sel];
    SendMessageW(state.hk_list, LB_DELETESTRING, sel, 0);
    let label = hk_list_label(sel, m, vk);
    SendMessageW(state.hk_list, LB_INSERTSTRING, sel, label.as_ptr() as LPARAM);
    SendMessageW(state.hk_list, LB_SETCURSEL, sel, 0);
}

unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            let code = (wparam >> 16) as u32;
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DlgState;
            if state.is_null() {
                return 0;
            }
            let state = &mut *state;
            match id {
                ID_OK => {
                    state.results = state.edits.iter().map(|&e| read_window_text(e)).collect();
                    state.check_vals = state
                        .checks
                        .iter()
                        .map(|&c| SendMessageW(c, BM_GETCHECK, 0, 0) == 1)
                        .collect();
                    state.ok = true;
                    state.done = true;
                }
                ID_CANCEL => state.done = true,
                ID_HK_ASSIGN => {
                    let sel = SendMessageW(state.hk_list, LB_GETCURSEL, 0, 0);
                    if sel >= 0 {
                        let hk = SendMessageW(state.hk_ctrl, HKM_GETHOTKEY, 0, 0) as u32;
                        let vk = hk & 0xff;
                        let m = hotkeyf_to_mod((hk >> 8) & 0xff);
                        state.hk_vals[sel as usize] = if vk == 0 { (0, 0) } else { (m, vk) };
                        hk_update_row(state, sel as usize);
                    }
                }
                ID_HK_CLEAR => {
                    let sel = SendMessageW(state.hk_list, LB_GETCURSEL, 0, 0);
                    if sel >= 0 {
                        state.hk_vals[sel as usize] = (0, 0);
                        SendMessageW(state.hk_ctrl, HKM_SETHOTKEY, 0, 0);
                        hk_update_row(state, sel as usize);
                    }
                }
                ID_HK_LIST if code == LBN_SELCHANGE => {
                    let sel = SendMessageW(state.hk_list, LB_GETCURSEL, 0, 0);
                    if sel >= 0 {
                        let (m, vk) = state.hk_vals[sel as usize];
                        let hf = (mod_to_hotkeyf(m) << 8) | vk;
                        SendMessageW(state.hk_ctrl, HKM_SETHOTKEY, hf as WPARAM, 0);
                    }
                }
                _ => {}
            }
            0
        }
        WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            if let Some(r) = ctl_color(msg, wparam as HDC) {
                return r;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => {
            if let Some(r) = erase_bkgnd(hwnd, wparam as HDC) {
                return r;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DlgState;
            if !state.is_null() {
                (*state).done = true;
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn register_dlg_class() {
    static mut DONE: bool = false;
    if DONE {
        return;
    }
    DONE = true;
    let name = wz("ClipMgrDlg");
    let mut wc: WNDCLASSW = zeroed();
    wc.lpfnWndProc = Some(dlg_proc);
    wc.hInstance = GetModuleHandleW(null());
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = (COLOR_BTNFACE + 1) as usize as HBRUSH;
    wc.lpszClassName = name.as_ptr();
    RegisterClassW(&wc);
}

// ダイアログ共通: 作成 → モーダルループ → 結果返却
unsafe fn run_modal(
    parent: HWND,
    title: &str,
    width: i32,
    height: i32,
    build: impl FnOnce(HWND, &mut DlgState, HFONT),
) -> Option<(Vec<String>, Vec<(u32, u32)>, Vec<bool>)> {
    register_dlg_class();
    let (font, dark) = with_app(|a| (a.font, a.dark));
    let mut state = DlgState {
        done: false,
        ok: false,
        edits: Vec::new(),
        results: Vec::new(),
        hk_list: null_mut(),
        hk_ctrl: null_mut(),
        hk_vals: Vec::new(),
        checks: Vec::new(),
        check_vals: Vec::new(),
    };

    // 親の中央に配置 (画面外にはみ出さないようワークエリア内にクランプ)
    let mut prc: RECT = zeroed();
    GetWindowRect(parent, &mut prc);
    let mut work: RECT = zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as *mut c_void, 0);
    let x = (prc.left + ((prc.right - prc.left) - width) / 2)
        .min(work.right - width)
        .max(work.left);
    let y = (prc.top + ((prc.bottom - prc.top) - height) / 2)
        .min(work.bottom - height)
        .max(work.top);

    let class = wz("ClipMgrDlg");
    let t = wz(title);
    let dlg = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class.as_ptr(),
        t.as_ptr(),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        x,
        y,
        width,
        height,
        parent,
        null_mut(),
        GetModuleHandleW(null()),
        &mut state as *mut DlgState as *mut c_void,
    );
    if dlg.is_null() {
        return None;
    }
    dwm_dark(dlg, dark);
    build(dlg, &mut state, font);

    EnableWindow(parent, 0);
    ShowWindow(dlg, SW_SHOW);
    if let Some(&first) = state.edits.first() {
        SetFocus(first);
        SendMessageW(first, EM_SETSEL, 0, -1);
    }

    let mut msg: MSG = zeroed();
    while !state.done {
        if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
            PostQuitMessage(0);
            break;
        }
        if IsDialogMessageW(dlg, &msg) == 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    EnableWindow(parent, 1);
    DestroyWindow(dlg);
    SetForegroundWindow(parent);
    if state.ok {
        Some((state.results, state.hk_vals, state.check_vals))
    } else {
        None
    }
}

unsafe fn create_dlg_control(
    dlg: HWND,
    class: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: i32,
    font: HFONT,
) -> HWND {
    let c = wz(class);
    let t = wz(text);
    let hwnd = CreateWindowExW(
        if class == "EDIT" || class == "LISTBOX" { WS_EX_CLIENTEDGE } else { 0 },
        c.as_ptr(),
        t.as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        x,
        y,
        w,
        h,
        dlg,
        id as usize as *mut c_void,
        GetModuleHandleW(null()),
        null(),
    );
    SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
    theme_control(hwnd, with_app(|a| a.dark));
    hwnd
}

// 定型文の追加・編集用テキスト入力ダイアログ
unsafe fn input_text_dialog(parent: HWND, title: &str, initial: &str) -> Option<String> {
    let initial = initial.to_string();
    let (results, _, _) = run_modal(parent, title, 480, 300, |dlg, state, font| {
        let mut rc: RECT = zeroed();
        GetClientRect(dlg, &mut rc);
        let w = rc.right;
        let h = rc.bottom;
        let edit = create_dlg_control(
            dlg,
            "EDIT",
            &initial,
            WS_TABSTOP | WS_VSCROLL | (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32,
            10, 10, w - 20, h - 60,
            -1, font,
        );
        create_dlg_control(dlg, "BUTTON", "OK", WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            w - 180, h - 40, 80, 28, ID_OK, font);
        create_dlg_control(dlg, "BUTTON", "キャンセル", WS_TABSTOP,
            w - 92, h - 40, 82, 28, ID_CANCEL, font);
        state.edits.push(edit);
    })?;
    results.into_iter().next()
}

struct SettingsResult {
    history_max: usize,
    snippet_max: usize,
    font_size: u32,
    topmost: bool,
    move_to_top: bool,
    hotkeys: Vec<(u32, u32)>,
}

// 設定ダイアログ
unsafe fn settings_dialog(parent: HWND) -> Option<SettingsResult> {
    let (h0, s0, f0, t0, m0, hk0) = with_app(|a| {
        (a.history_max, a.snippet_max, a.font_size, a.topmost, a.move_to_top, a.hotkeys)
    });
    let (results, hk_vals, checks) = run_modal(parent, "設定", 420, 626, |dlg, state, font| {
        let mut rc: RECT = zeroed();
        GetClientRect(dlg, &mut rc);
        let w = rc.right;
        let h = rc.bottom;

        create_dlg_control(dlg, "STATIC", "履歴の保持数 (1〜1000):", 0, 14, 16, 190, 20, -1, font);
        let e1 = create_dlg_control(dlg, "EDIT", &h0.to_string(),
            WS_TABSTOP | ES_NUMBER as u32, 210, 12, 100, 24, -1, font);
        create_dlg_control(dlg, "STATIC", "定型文の保持数 (1〜1000):", 0, 14, 50, 190, 20, -1, font);
        let e2 = create_dlg_control(dlg, "EDIT", &s0.to_string(),
            WS_TABSTOP | ES_NUMBER as u32, 210, 46, 100, 24, -1, font);
        create_dlg_control(dlg, "STATIC", "項目のフォントサイズ (6〜40):", 0, 14, 84, 190, 20, -1, font);
        let e3 = create_dlg_control(dlg, "EDIT", &f0.to_string(),
            WS_TABSTOP | ES_NUMBER as u32, 210, 80, 100, 24, -1, font);

        // ラベルはテーマ任せにせずSTATICで描画 (ダークモードで文字が背景に同化するのを防ぐ)
        let chk1 = create_dlg_control(dlg, "BUTTON", "",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32, 14, 114, 20, 22, -1, font);
        create_dlg_control(dlg, "STATIC", "ウィンドウを常に手前に表示", 0, 38, 117, w - 52, 20, -1, font);
        SendMessageW(chk1, BM_SETCHECK, if t0 { 1 } else { 0 }, 0);
        let chk2 = create_dlg_control(dlg, "BUTTON", "",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32, 14, 140, 20, 22, -1, font);
        create_dlg_control(dlg, "STATIC", "同じ内容を再コピーしたら履歴の先頭へ移動",
            0, 38, 143, w - 52, 20, -1, font);
        SendMessageW(chk2, BM_SETCHECK, if m0 { 1 } else { 0 }, 0);

        create_dlg_control(dlg, "STATIC", "ショートカットキー (一覧から選択 → キーを入力 → 割り当て):",
            0, 14, 174, w - 28, 20, -1, font);
        let list = create_dlg_control(dlg, "LISTBOX", "",
            WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_USETABSTOPS) as u32,
            14, 196, w - 28, h - 306, ID_HK_LIST, font);
        let hkc = create_dlg_control(dlg, "msctls_hotkey32", "", WS_TABSTOP,
            14, h - 102, w - 180, 24, -1, font);
        create_dlg_control(dlg, "BUTTON", "割り当て", WS_TABSTOP,
            w - 158, h - 104, 72, 26, ID_HK_ASSIGN, font);
        create_dlg_control(dlg, "BUTTON", "解除", WS_TABSTOP,
            w - 80, h - 104, 64, 26, ID_HK_CLEAR, font);

        create_dlg_control(dlg, "BUTTON", "OK", WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            w - 180, h - 42, 80, 28, ID_OK, font);
        create_dlg_control(dlg, "BUTTON", "キャンセル", WS_TABSTOP,
            w - 92, h - 42, 82, 28, ID_CANCEL, font);

        // タブストップ設定 + 一覧を構築
        let tab_px: i32 = 150;
        SendMessageW(list, LB_SETTABSTOPS, 1, &tab_px as *const i32 as LPARAM);
        for (i, (m, vk)) in hk0.iter().enumerate() {
            let label = hk_list_label(i, *m, *vk);
            SendMessageW(list, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
        }
        SendMessageW(list, LB_SETCURSEL, 0, 0);
        let (m, vk) = hk0[0];
        SendMessageW(hkc, HKM_SETHOTKEY, ((mod_to_hotkeyf(m) << 8) | vk) as WPARAM, 0);

        state.edits.push(e1);
        state.edits.push(e2);
        state.edits.push(e3);
        state.hk_list = list;
        state.hk_ctrl = hkc;
        state.hk_vals = hk0.to_vec();
        state.checks.push(chk1);
        state.checks.push(chk2);
    })?;
    let h: usize = results.first()?.trim().parse().ok()?;
    let s: usize = results.get(1)?.trim().parse().ok()?;
    let f: u32 = results.get(2)?.trim().parse().ok()?;
    if !(1..=1000).contains(&h) || !(1..=1000).contains(&s) || !(6..=40).contains(&f) {
        return None;
    }
    Some(SettingsResult {
        history_max: h,
        snippet_max: s,
        font_size: f,
        topmost: checks.first().copied().unwrap_or(true),
        move_to_top: checks.get(1).copied().unwrap_or(true),
        hotkeys: hk_vals,
    })
}

// ============ リストボックスのサブクラス (Enter/Delete/Escキー対応) ============

static mut OLD_LIST_PROC: WNDPROC = None;

unsafe extern "system" fn list_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN {
        let parent = GetParent(hwnd);
        match wparam as u16 {
            x if x == VK_RETURN => {
                SendMessageW(parent, WM_COMMAND, ID_PASTE as WPARAM, 0);
                return 0;
            }
            x if x == VK_DELETE => {
                SendMessageW(parent, WM_COMMAND, ID_DELETE as WPARAM, 0);
                return 0;
            }
            x if x == VK_ESCAPE => {
                ShowWindow(parent, SW_HIDE);
                return 0;
            }
            _ => {}
        }
    }
    CallWindowProcW(OLD_LIST_PROC, hwnd, msg, wparam, lparam)
}

// ============ 画像モード(メイン内サムネイルパネル) ============

const IMG_CELL: i32 = 96; // セル一辺(余白込み)
const IMG_PAD: i32 = 4;
// 画像モードで必要なクライアント幅(3列ぶん)。これに合わせてウィンドウを広げる
const IMG_PANEL_W: i32 = IMG_PAD + 3 * IMG_CELL;

// パネル幅から列数を算出 (最低1列)
unsafe fn img_cols(panel_w: i32) -> i32 {
    ((panel_w - IMG_PAD) / IMG_CELL).max(1)
}

unsafe fn img_cell_origin(i: usize, cols: i32) -> (i32, i32) {
    let col = (i as i32) % cols;
    let row = (i as i32) / cols;
    (IMG_PAD + col * IMG_CELL, IMG_PAD + row * IMG_CELL)
}

// クリック座標 → 画像インデックス
unsafe fn img_hit_test(x: i32, y: i32, count: usize, cols: i32) -> Option<usize> {
    if x < IMG_PAD || y < IMG_PAD {
        return None;
    }
    let col = (x - IMG_PAD) / IMG_CELL;
    let row = (y - IMG_PAD) / IMG_CELL;
    if col >= cols {
        return None;
    }
    let idx = (row * cols + col) as usize;
    if idx < count {
        Some(idx)
    } else {
        None
    }
}

unsafe fn paint_image_panel(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);
    let cols = img_cols(rc.right);

    let dark = with_app(|a| a.dark);
    let bg = CreateSolidBrush(if dark { DARK_BG_WINDOW } else { 0x00F0F0F0 });
    FillRect(hdc, &rc, bg);
    DeleteObject(bg as _);

    let count = with_app(|a| a.images.len());
    if count == 0 {
        let txt = wz("画像はありません (画像をコピーすると追加されます)");
        SetBkMode(hdc, 1); // TRANSPARENT
        SetTextColor(hdc, if dark { DARK_TEXT } else { 0x00404040 });
        let mut tr = RECT { left: 0, top: 40, right: rc.right, bottom: 80 };
        DrawTextW(hdc, txt.as_ptr(), -1, &mut tr, DT_CENTER | DT_SINGLELINE);
        EndPaint(hwnd, &ps);
        return;
    }

    let sel = with_app(|a| a.img_selected);
    let cell_bg = CreateSolidBrush(if dark { DARK_BG_CTRL } else { 0x00FFFFFF });
    let sel_brush = CreateSolidBrush(0x00D07820); // 選択枠(青系)
    let memdc = CreateCompatibleDC(hdc);

    for i in 0..count {
        let (ox, oy) = img_cell_origin(i, cols);
        let cell = RECT {
            left: ox,
            top: oy,
            right: ox + IMG_CELL - IMG_PAD,
            bottom: oy + IMG_CELL - IMG_PAD,
        };
        FillRect(hdc, &cell, cell_bg);
        let (thumb, tw, th) = with_app(|a| {
            let it = &a.images[i];
            (it.thumb, it.tw, it.th)
        });
        let inner = IMG_CELL - IMG_PAD;
        let dx = ox + (inner - tw) / 2;
        let dy = oy + (inner - th) / 2;
        let old = SelectObject(memdc, thumb as _);
        BitBlt(hdc, dx, dy, tw, th, memdc, 0, 0, SRCCOPY);
        SelectObject(memdc, old);
        if i as isize == sel {
            let mut f = cell;
            FrameRect(hdc, &f, sel_brush);
            f.left += 1;
            f.top += 1;
            f.right -= 1;
            f.bottom -= 1;
            FrameRect(hdc, &f, sel_brush);
        }
    }
    DeleteDC(memdc);
    DeleteObject(cell_bg as _);
    DeleteObject(sel_brush as _);
    EndPaint(hwnd, &ps);
}

// 選択中の画像を貼り付け (main = メインウィンドウ)
unsafe fn img_paste_selected(main: HWND) {
    let (sel, count) = with_app(|a| (a.img_selected, a.images.len()));
    if sel < 0 || sel as usize >= count {
        return;
    }
    paste_image(main, sel as usize);
}

unsafe fn img_delete_selected() {
    with_app(|a| {
        let sel = a.img_selected;
        if sel >= 0 && (sel as usize) < a.images.len() {
            a.images.remove(sel as usize);
            if a.images.is_empty() {
                a.img_selected = -1;
            } else if sel as usize >= a.images.len() {
                a.img_selected = a.images.len() as isize - 1;
            }
        }
    });
}

// サムネイルパネルのウィンドウプロシージャ (描画とクリック選択のみ)
unsafe extern "system" fn img_panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let cols = img_cols(rc.right);
            let count = with_app(|a| a.images.len());
            if let Some(idx) = img_hit_test(x, y, count, cols) {
                with_app(|a| a.img_selected = idx as isize);
                InvalidateRect(hwnd, null(), 1);
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let cols = img_cols(rc.right);
            let count = with_app(|a| a.images.len());
            if let Some(idx) = img_hit_test(x, y, count, cols) {
                with_app(|a| a.img_selected = idx as isize);
                img_paste_selected(GetParent(hwnd));
            }
            0
        }
        WM_PAINT => {
            paint_image_panel(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn register_img_panel_class() {
    static mut DONE: bool = false;
    if DONE {
        return;
    }
    DONE = true;
    let name = wz("ClipMgrImgPanel");
    let mut wc: WNDCLASSW = zeroed();
    wc.lpfnWndProc = Some(img_panel_proc);
    wc.hInstance = GetModuleHandleW(null());
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = (COLOR_BTNFACE + 1) as usize as HBRUSH;
    wc.lpszClassName = name.as_ptr();
    wc.style = CS_DBLCLKS; // ダブルクリックを受け取る
    RegisterClassW(&wc);
}

// ============ メインウィンドウ ============

unsafe fn create_list_font(hwnd: HWND, size_pt: u32) -> HFONT {
    let mut ncm: NONCLIENTMETRICSW = zeroed();
    ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
    SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut c_void, 0);
    let mut lf = ncm.lfMessageFont;
    let dpi = GetDpiForWindow(hwnd);
    let dpi = if dpi == 0 { 96 } else { dpi };
    lf.lfHeight = -((size_pt * dpi / 72) as i32);
    CreateFontIndirectW(&lf)
}

unsafe fn create_controls(hwnd: HWND) {
    let inst = GetModuleHandleW(null());
    let make = |class: &str, text: &str, style: u32, id: i32| -> HWND {
        let c = wz(class);
        let t = wz(text);
        CreateWindowExW(
            if class == "LISTBOX" { WS_EX_CLIENTEDGE } else { 0 },
            c.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0, 0, 10, 10,
            hwnd,
            id as usize as *mut c_void,
            inst,
            null(),
        )
    };

    let lbl_mode = make("STATIC", "モード:", 0, -1);
    let btn_mode = make("BUTTON", "履歴", WS_TABSTOP, ID_MODE);
    let list = make(
        "LISTBOX",
        "",
        WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32,
        ID_LIST,
    );
    let btn_paste = make("BUTTON", "貼り付け", WS_TABSTOP, ID_PASTE);
    let btn_to_snip = make("BUTTON", "定型文に登録", WS_TABSTOP, ID_TO_SNIP);
    let btn_add = make("BUTTON", "追加", WS_TABSTOP, ID_ADD);
    let btn_edit = make("BUTTON", "編集", WS_TABSTOP, ID_EDIT);
    let btn_delete = make("BUTTON", "削除", WS_TABSTOP, ID_DELETE);
    let btn_settings = make("BUTTON", "設定", WS_TABSTOP, ID_SETTINGS);
    // 画像モードでのみ表示する「全消去」ボタン (履歴/定型文モードでは非表示)
    let btn_images = make("BUTTON", "全消去", WS_TABSTOP, ID_IMAGES);

    // 画像モードのサムネイル描画パネル (初期は非表示)
    register_img_panel_class();
    let img_panel = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wz("ClipMgrImgPanel").as_ptr(),
        null(),
        WS_CHILD,
        0, 0, 10, 10,
        hwnd,
        null_mut(),
        inst,
        null(),
    );

    // システムのメッセージフォント(日本語UIフォント)を適用
    let mut ncm: NONCLIENTMETRICSW = zeroed();
    ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
    SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut c_void, 0);
    let font = CreateFontIndirectW(&ncm.lfMessageFont);
    for h in [
        lbl_mode, btn_mode, btn_paste, btn_to_snip, btn_add, btn_edit, btn_delete, btn_settings,
        btn_images,
    ] {
        SendMessageW(h, WM_SETFONT, font as usize, 1);
    }
    // リストは設定されたフォントサイズを使用
    let size_pt = with_app(|a| a.font_size);
    let list_font = create_list_font(hwnd, size_pt);
    SendMessageW(list, WM_SETFONT, list_font as usize, 1);

    // Enter/Delete/Escキーを拾うためにリストボックスをサブクラス化
    let old = SetWindowLongPtrW(list, GWLP_WNDPROC, list_proc as *const () as isize);
    OLD_LIST_PROC = std::mem::transmute::<isize, WNDPROC>(old);

    with_app(|a| {
        a.list = list;
        a.lbl_mode = lbl_mode;
        a.btn_mode = btn_mode;
        a.btn_paste = btn_paste;
        a.btn_to_snip = btn_to_snip;
        a.btn_add = btn_add;
        a.btn_edit = btn_edit;
        a.btn_delete = btn_delete;
        a.btn_settings = btn_settings;
        a.btn_images = btn_images;
        a.img_panel = img_panel;
        a.font = font;
        a.list_font = list_font;
    });
}

unsafe fn add_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    // 埋め込みアイコン(ID=1)、無ければ標準アイコン
    nid.hIcon = LoadIconW(GetModuleHandleW(null()), 1 as *const u16);
    if nid.hIcon.is_null() {
        nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
    }
    let tip = wz("クリップボード履歴");
    let n = tip.len().min(nid.szTip.len());
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    AppendMenuW(menu, MF_STRING, ID_TRAY_SHOW as usize, wz("表示(&S)").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT as usize, wz("終了(&X)").as_ptr());
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    match cmd {
        x if x == ID_TRAY_SHOW => show_main(hwnd),
        x if x == ID_TRAY_EXIT => {
            save_state();
            remove_tray_icon(hwnd);
            DestroyWindow(hwnd);
        }
        _ => {}
    }
}

unsafe fn snippet_full_warning(hwnd: HWND) {
    let max = with_app(|a| a.snippet_max);
    message_box(
        hwnd,
        &format!(
            "定型文は最大{}件です。\n設定で上限を変更するか、不要な定型文を削除してください。",
            max
        ),
        "登録できません",
    );
}

unsafe fn handle_command(hwnd: HWND, id: i32, code: u32) {
    match id {
        ID_MODE => {
            // 押すたびに履歴⇔定型文を切り替え
            with_app(|a| {
                a.tab = match a.tab {
                    Tab::History => Tab::Snippet,
                    Tab::Snippet => Tab::Image,
                    Tab::Image => Tab::History,
                }
            });
            update_tab_ui(hwnd);
        }
        ID_LIST if code == LBN_DBLCLK as u32 => do_paste(hwnd),
        ID_PASTE => {
            if with_app(|a| a.tab == Tab::Image) {
                img_paste_selected(hwnd);
            } else {
                do_paste(hwnd);
            }
        }
        ID_TO_SNIP => {
            let Some(sel) = selected_index() else { return };
            let (text, full) = with_app(|a| {
                (a.history.get(sel).cloned(), a.snippets.len() >= a.snippet_max)
            });
            let Some(text) = text else { return };
            if full {
                snippet_full_warning(hwnd);
                return;
            }
            with_app(|a| {
                a.snippets.push(text);
                a.tab = Tab::Snippet;
            });
            save_state();
            update_tab_ui(hwnd);
            // 追加した項目(末尾)を選択
            let list = with_app(|a| a.list);
            let count = SendMessageW(list, LB_GETCOUNT, 0, 0);
            if count > 0 {
                SendMessageW(list, LB_SETCURSEL, (count - 1) as WPARAM, 0);
            }
        }
        ID_ADD => {
            if with_app(|a| a.snippets.len() >= a.snippet_max) {
                snippet_full_warning(hwnd);
                return;
            }
            if let Some(text) = input_text_dialog(hwnd, "定型文の追加", "") {
                if !text.is_empty() {
                    with_app(|a| a.snippets.push(text));
                    save_state();
                    refresh_list(false);
                }
            }
        }
        ID_EDIT => {
            let Some(sel) = selected_index() else { return };
            let Some(current) = with_app(|a| a.snippets.get(sel).cloned()) else { return };
            if let Some(text) = input_text_dialog(hwnd, "定型文の編集", &current) {
                if !text.is_empty() {
                    with_app(|a| {
                        if sel < a.snippets.len() {
                            a.snippets[sel] = text;
                        }
                    });
                    save_state();
                    refresh_list(true);
                }
            }
        }
        ID_DELETE => {
            if with_app(|a| a.tab == Tab::Image) {
                img_delete_selected();
                let panel = with_app(|a| a.img_panel);
                InvalidateRect(panel, null(), 1);
                return;
            }
            let Some(sel) = selected_index() else { return };
            with_app(|a| {
                let items = match a.tab {
                    Tab::History => &mut a.history,
                    Tab::Snippet => &mut a.snippets,
                    Tab::Image => return, // 上で処理済み(到達しない)
                };
                if sel < items.len() {
                    items.remove(sel);
                }
            });
            save_state();
            refresh_list(true);
        }
        ID_IMAGES => {
            // 画像モードの「全消去」ボタン
            with_app(|a| {
                a.images.clear();
                a.img_selected = -1;
            });
            let panel = with_app(|a| a.img_panel);
            InvalidateRect(panel, null(), 1);
        }
        ID_SETTINGS => {
            if let Some(r) = settings_dialog(hwnd) {
                let font_changed = with_app(|a| {
                    a.history_max = r.history_max;
                    a.snippet_max = r.snippet_max;
                    a.topmost = r.topmost;
                    a.move_to_top = r.move_to_top;
                    let changed = a.font_size != r.font_size;
                    a.font_size = r.font_size;
                    a.history.truncate(r.history_max);
                    for (i, v) in r.hotkeys.iter().enumerate() {
                        if i < HK_COUNT {
                            a.hotkeys[i] = *v;
                        }
                    }
                    changed
                });
                if font_changed {
                    let size_pt = with_app(|a| a.font_size);
                    let new_font = create_list_font(hwnd, size_pt);
                    let (list, old) = with_app(|a| {
                        let old = a.list_font;
                        a.list_font = new_font;
                        (a.list, old)
                    });
                    SendMessageW(list, WM_SETFONT, new_font as usize, 1);
                    if !old.is_null() {
                        DeleteObject(old as _);
                    }
                }
                save_state();
                let failed = apply_hotkeys(hwnd);
                if !failed.is_empty() {
                    let names: Vec<String> = failed
                        .iter()
                        .map(|&i| {
                            let (m, vk) = with_app(|a| a.hotkeys[i]);
                            format!("・{} ({})", action_label(i), key_name(m, vk))
                        })
                        .collect();
                    message_box(
                        hwnd,
                        &format!(
                            "以下のショートカットキーは他のアプリで使用中のため登録できませんでした:\n{}",
                            names.join("\n")
                        ),
                        "ショートカットキー",
                    );
                }
                refresh_list(true);
                layout(hwnd);
                apply_topmost(hwnd, false);
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            // 非表示のまま作成されるとWM_SIZEが来ないため初回レイアウトを明示実行
            layout(hwnd);
            apply_theme(hwnd);
            // Chrome 等が WM_CLIPBOARDUPDATE を発火しない場合のポーリング
            SetTimer(hwnd, ID_TIMER_CLIP, 500, None);
            0
        }
        WM_SIZE => {
            if wparam != SIZE_MINIMIZED as usize {
                layout(hwnd);
            }
            0
        }
        WM_GETMINMAXINFO => {
            let mmi = lparam as *mut MINMAXINFO;
            (*mmi).ptMinTrackSize = POINT { x: 230, y: 340 };
            0
        }
        WM_COMMAND => {
            handle_command(hwnd, (wparam & 0xffff) as i32, (wparam >> 16) as u32);
            0
        }
        WM_HOTKEY => {
            let idx = wparam as i32 - HK_ID_BASE;
            if (0..HK_COUNT as i32).contains(&idx) {
                handle_hotkey(hwnd, idx as usize);
            }
            0
        }
        WM_TRAY => {
            match (lparam & 0xffff) as u32 {
                WM_LBUTTONUP => show_main(hwnd),
                WM_RBUTTONUP => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_SHOW_SELF => {
            show_main(hwnd);
            0
        }
        WM_TIMER if wparam == ID_TIMER_CLIP => {
            // WM_CLIPBOARDUPDATE を発火しないアプリ(Chrome等)の画像コピーを捕捉
            let seq = GetClipboardSequenceNumber();
            let changed = with_app(|a| {
                if a.clip_seq != seq { a.clip_seq = seq; true } else { false }
            });
            if changed {
                // テキストは WM_CLIPBOARDUPDATE で処理済みなので画像だけ確認
                let cf_imgpng = with_app(|a| a.cf_imgpng);
                let has_img = IsClipboardFormatAvailable(CF_BITMAP) != 0
                    || IsClipboardFormatAvailable(CF_DIB) != 0
                    || (cf_imgpng != 0 && IsClipboardFormatAvailable(cf_imgpng) != 0);
                if has_img && !with_app(|a| a.suppress) {
                    if let Some(item) = capture_clipboard_image(hwnd) {
                        let added = with_app(|a| {
                            if a.images.first().map(|x| x.png == item.png).unwrap_or(false) {
                                false
                            } else {
                                a.images.insert(0, item);
                                a.images.truncate(IMAGE_MAX);
                                a.img_selected = 0;
                                true
                            }
                        });
                        if added {
                            let panel = with_app(|a| a.img_panel);
                            if !panel.is_null() {
                                InvalidateRect(panel, null(), 1);
                            }
                        }
                    }
                }
            }
            0
        }
        WM_CLIPBOARDUPDATE => {
            // clip_seq をここで同期しておくことで、タイマーが同じ変化を二重処理しない
            let seq = GetClipboardSequenceNumber();
            with_app(|a| a.clip_seq = seq);
            let suppress = with_app(|a| {
                let s = a.suppress;
                a.suppress = false;
                s
            });
            if !suppress {
                if let Some(text) = get_clipboard_text(hwnd) {
                    if !text.is_empty() {
                        let changed = with_app(|a| {
                            // 履歴内に同じ内容が既にある場合: 設定に応じて先頭へ移動 or スキップ
                            if let Some(pos) = a.history.iter().position(|h| h == &text) {
                                if a.move_to_top && pos > 0 {
                                    let item = a.history.remove(pos);
                                    a.history.insert(0, item);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                a.history.insert(0, text);
                                let max = a.history_max;
                                a.history.truncate(max);
                                true
                            }
                        });
                        if changed {
                            save_state();
                            // 非表示中でもリストへ即反映 (次回表示時に最新の状態)
                            if with_app(|a| a.tab == Tab::History) {
                                refresh_list(false);
                            }
                        }
                    }
                } else if let Some(item) = capture_clipboard_image(hwnd) {
                    // テキストが無く画像があれば画像履歴へ (メモリ保持・非永続)
                    // 直前と同じ画像はスキップ (SetImage等が複数回通知するための重複対策)
                    let added = with_app(|a| {
                        if a.images.first().map(|x| x.png == item.png).unwrap_or(false) {
                            false
                        } else {
                            a.images.insert(0, item);
                            a.images.truncate(IMAGE_MAX);
                            a.img_selected = 0;
                            true
                        }
                    });
                    if added {
                        let panel = with_app(|a| a.img_panel);
                        if !panel.is_null() {
                            InvalidateRect(panel, null(), 1);
                        }
                    }
                }
            }
            0
        }
        WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            if let Some(r) = ctl_color(msg, wparam as HDC) {
                return r;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => {
            if let Some(r) = erase_bkgnd(hwnd, wparam as HDC) {
                return r;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_SETTINGCHANGE => {
            // OSのライト/ダークテーマ切り替えに追従
            if lparam != 0 {
                let s = lparam as *const u16;
                let mut len = 0;
                while len < 64 && *s.add(len) != 0 {
                    len += 1;
                }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
                if name == "ImmersiveColorSet" {
                    apply_theme(hwnd);
                }
            }
            0
        }
        WM_CLOSE => {
            // 閉じるボタンは非表示化(常駐継続)。終了はトレイメニューから。
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    unsafe {
        SetProcessDPIAware();

        // 多重起動抑制: 名前付きミューテックス + 既存ウィンドウの表示
        let class_name = wz("ClipMgrMainWnd");
        let mutex_name = wz("Local\\ClipboardManagerSingleton");
        CreateMutexW(null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(class_name.as_ptr(), null());
            if !existing.is_null() {
                PostMessageW(existing, WM_SHOW_SELF, 0, 0);
            }
            return;
        }

        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_HOTKEY_CLASS | ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&icc);

        // GDI+ 初期化 (画像のPNGエンコード/デコード用)
        let mut gdip_token: usize = 0;
        let gdip_input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        GdiplusStartup(&mut gdip_token, &gdip_input, null_mut());

        load_state();
        // 画像貼り付け用の "PNG" クリップボード形式を登録
        let cf_png = RegisterClipboardFormatW(wz("PNG").as_ptr());
        // Chrome/Edge が "画像をコピー" で置く形式を登録
        let cf_imgpng = RegisterClipboardFormatW(wz("image/png").as_ptr());
        with_app(|a| { a.cf_png = cf_png; a.cf_imgpng = cf_imgpng; });

        let inst = GetModuleHandleW(null());
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = inst;
        wc.hIcon = LoadIconW(inst, 1 as *const u16); // 埋め込みアイコン(ID=1)
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hbrBackground = (COLOR_BTNFACE + 1) as usize as HBRUSH;
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        // 初期位置: 画面右側の中段 (以降はドラッグで移動した位置を維持)
        let (win_w, win_h) = (250, 520);
        let mut work: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as *mut c_void, 0);
        let init_x = (work.right - win_w - 20).max(work.left);
        let init_y = ((work.top + work.bottom) / 2 - win_h / 2).max(work.top);

        let title = wz("クリップボード履歴");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW, // 最小化・最大化・リサイズ可能な通常ウィンドウ
            init_x,
            init_y,
            win_w,
            win_h,
            null_mut(),
            null_mut(),
            inst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }

        AddClipboardFormatListener(hwnd);
        apply_hotkeys(hwnd);
        add_tray_icon(hwnd);
        // 貼り付け先追跡用のフォアグラウンド監視フック
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        // 起動時はウィンドウを表示した状態で開始
        show_main(hwnd);

        // 起動時は非表示でトレイ常駐 (ウィンドウは作成済みなのでホットキー表示は低遅延)
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        save_state();
    }
}
