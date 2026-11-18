//! Win32 implementation of a floating, non-invasive taskbar.
//!
//! Safety boundary: we do not replace Explorer, install global hooks, reserve work
//! area, change Windows settings, or touch another process's memory.

use crate::model::{self, Action};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SYSTEMTIME, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_ESCAPE, VK_LWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: &str = "RustTaskbarPrototypeWindow";
const CLOCK_TIMER: usize = 1;
const FALLBACK_TIMER: usize = 2;
const PANEL_WIDTH_DIP: i32 = 940;
const PANEL_HEIGHT_DIP: i32 = 56;
const MARGIN_DIP: i32 = 12;

/// An owned copy of another window's icon. Shared class/window handles must
/// never be passed directly to DestroyIcon.
struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: CopyIcon creates a distinct owned handle.
        unsafe { DestroyIcon(self.0); }
    }
}

struct OpenWindow {
    hwnd: HWND,
    title: String,
    icon: Option<OwnedIcon>,
}

#[derive(Default)]
struct AppState {
    windows: Vec<OpenWindow>,
    shell_message: u32,
    dpi: u32,
    hovered: Option<Action>,
    tracking_mouse: bool,
}

fn wide(input: &str) -> Vec<u16> {
    input.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

pub fn show_error(error: &str) {
    let message = wide(error);
    let caption = wide("rust-taskbar startup error");
    // SAFETY: Both UTF-16 buffers are terminated and live for the call.
    unsafe {
        MessageBoxW(null_mut(), message.as_ptr(), caption.as_ptr(), MB_ICONERROR | MB_OK);
    }
}

pub fn run() -> Result<(), String> {
    // SAFETY: Calls below run on one UI thread, with a boxed state that remains
    // allocated until the message pump ends.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let module = GetModuleHandleW(null());
        if module.is_null() {
            return Err(win32_error("GetModuleHandleW"));
        }

        let class_name = wide(CLASS_NAME);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class_name.as_ptr(),
            ..zeroed()
        };
        if RegisterClassW(&class) == 0 {
            return Err(win32_error("RegisterClassW"));
        }

        let mut state = Box::<AppState>::default();
        let title = wide("Rust Taskbar (prototype)");
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            700,
            56,
            null_mut(),
            null_mut(),
            module,
            (&mut *state as *mut AppState).cast::<c_void>(),
        );
        if hwnd.is_null() {
            return Err(win32_error("CreateWindowExW"));
        }

        state.dpi = GetDpiForWindow(hwnd).max(96);
        state.shell_message = RegisterWindowMessageW(wide("SHELLHOOK").as_ptr());
        if state.shell_message != 0 && RegisterShellHookWindow(hwnd) == 0 {
            // The window is still usable, but new window notifications may lag.
            eprintln!("RegisterShellHookWindow failed; falling back to manual refresh");
            state.shell_message = 0;
        }

        refresh_windows(hwnd, &mut state);
        if state.shell_message == 0 {
            // Only when shell notifications are unavailable: bounded fallback polling.
            SetTimer(hwnd, FALLBACK_TIMER, 5_000, None);
        }
        position_window(hwnd, state.dpi);
        schedule_clock(hwnd);

        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        UpdateWindow(hwnd);

        let mut message: MSG = zeroed();
        loop {
            let status = GetMessageW(&mut message, null_mut(), 0, 0);
            if status == -1 {
                return Err(win32_error("GetMessageW"));
            }
            if status == 0 {
                return Ok(());
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Called by EnumWindows on the same UI thread. The LPARAM points to a Vec
/// owned by refresh_windows and cannot outlive this synchronous callback.
unsafe extern "system" fn enumerate_window(hwnd: HWND, parameter: LPARAM) -> i32 {
    let result = &mut *(parameter as *mut Vec<OpenWindow>);
    if IsWindowVisible(hwnd) == 0
        || GetWindow(hwnd, GW_OWNER) != null_mut()
        || (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW) != 0
    {
        return 1;
    }

    let mut class_name = [0u16; 96];
    let count = GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32);
    if count > 0 {
        let name = String::from_utf16_lossy(&class_name[..count as usize]);
        if matches!(name.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | CLASS_NAME) {
            return 1;
        }
    }

    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        (&mut cloaked as *mut u32).cast::<c_void>(),
        size_of::<u32>() as u32,
    ) >= 0
        && cloaked != 0
    {
        return 1;
    }

    // Cap title lengths to avoid allocating untrusted window-provided lengths.
    let max_title = GetWindowTextLengthW(hwnd).clamp(0, 255) as usize;
    if max_title == 0 {
        return 1;
    }
    let mut text = vec![0u16; max_title + 1];
    let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
    if length <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&text[..length as usize]);
    if !title.trim().is_empty() {
        result.push(OpenWindow { hwnd, title, icon: None });
    }
    1
}

/// Obtain a small icon without hanging if a target app stops responding.
/// WM_GETICON is optional; class icons are used as a documented fallback.
unsafe fn copy_window_icon(hwnd: HWND) -> Option<OwnedIcon> {
    let mut raw: usize = 0;
    for icon_kind in [2usize, 0usize, 1usize] {
        let delivered = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            icon_kind,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            30,
            &mut raw,
        );
        if delivered != 0 && raw != 0 {
            break;
        }
    }
    if raw == 0 {
        raw = GetClassLongPtrW(hwnd, GCLP_HICONSM);
    }
    if raw == 0 {
        raw = GetClassLongPtrW(hwnd, GCLP_HICON);
    }
    if raw == 0 {
        return None;
    }
    let icon = CopyIcon(raw as HICON);
    (!icon.is_null()).then_some(OwnedIcon(icon))
}

unsafe fn refresh_windows(hwnd: HWND, state: &mut AppState) {
    let mut windows: Vec<OpenWindow> = Vec::with_capacity(32);
    EnumWindows(Some(enumerate_window), (&mut windows as *mut Vec<OpenWindow>) as isize);
    let changed = windows.len() != state.windows.len()
        || windows
            .iter()
            .zip(state.windows.iter())
            .any(|(a, b)| a.hwnd != b.hwnd || a.title != b.title);
    if changed {
        // Reuse copied icon handles for unchanged HWNDs. No class/window icon
        // is destroyed and no WM_GETICON is sent during ordinary WM_PAINT.
        let mut previous = std::mem::take(&mut state.windows);
        for window in &mut windows {
            if let Some(old) = previous.iter_mut().find(|old| old.hwnd == window.hwnd) {
                window.icon = old.icon.take();
            } else {
                window.icon = copy_window_icon(window.hwnd);
            }
        }
        state.windows = windows;
    }
    // The foreground HWND may change even when the window list is identical.
    InvalidateRect(hwnd, null(), 0);
}

unsafe fn position_window(hwnd: HWND, dpi: u32) {
    // SPI_GETWORKAREA respects the existing Explorer taskbar on the primary display.
    // This MVP intentionally does not register as a second appbar.
    let mut work_area: RECT = zeroed();
    if SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        (&mut work_area as *mut RECT).cast::<c_void>(),
        0,
    ) == 0
    {
        work_area.right = GetSystemMetrics(SM_CXSCREEN);
        work_area.bottom = GetSystemMetrics(SM_CYSCREEN);
    }
    let available_width = (work_area.right - work_area.left).max(0);
    let width = model::scale(PANEL_WIDTH_DIP, dpi).min((available_width - 2 * model::scale(MARGIN_DIP, dpi)).max(300));
    let height = model::scale(PANEL_HEIGHT_DIP, dpi);
    let x = work_area.left + (available_width - width) / 2;
    let y = work_area.bottom - height - model::scale(MARGIN_DIP, dpi);
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

unsafe fn schedule_clock(hwnd: HWND) {
    let mut time: SYSTEMTIME = zeroed();
    GetLocalTime(&mut time);
    let elapsed = u32::from(time.wSecond) * 1_000 + u32::from(time.wMilliseconds);
    let wait = (60_000 - elapsed).max(50);
    SetTimer(hwnd, CLOCK_TIMER, wait, None);
}

unsafe fn send_shortcut(vk: u16) {
    // Windows synthesizes the native Start/Search/Task View experience.
    // We never emulate those shell interfaces or change any registry keys.
    let keys = [
        (VK_LWIN, 0),
        (vk, 0),
        (vk, KEYEVENTF_KEYUP),
        (VK_LWIN, KEYEVENTF_KEYUP),
    ];
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&(key, flags)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();
    SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
}

unsafe fn open_start() {
    // Sending a plain Win key press/release opens the native Start menu.
    let keys = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    SendInput(keys.len() as u32, keys.as_ptr(), size_of::<INPUT>() as i32);
}


#[derive(Clone, Copy)]
enum Command {
    Start,
    Search,
    AllWindows,
    Window(HWND),
    Exit,
}

/// Resolve the click to a value before calling any Win32 function that may
/// synchronously re-enter window_proc (DestroyWindow/ShowWindow).
unsafe fn resolve_click(hwnd: HWND, state: &AppState, x: i32, y: i32) -> Option<Command> {
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    let layout = model::layout(client.right, client.bottom, state.dpi, state.windows.len());
    let action = model::hit_test(&layout, x, y);
    match action {
        Some(Action::Start) => Some(Command::Start),
        Some(Action::Search) => Some(Command::Search),
        Some(Action::AllWindows) => Some(Command::AllWindows),
        Some(Action::Window(index)) => state.windows.get(index).map(|window| Command::Window(window.hwnd)),
        Some(Action::Exit) => Some(Command::Exit),
        None => None,
    }
}

unsafe fn execute_command(hwnd: HWND, command: Command) {
    match command {
        Command::Start => open_start(),
        Command::Search => send_shortcut(b'S' as u16),
        Command::AllWindows => send_shortcut(0x09), // Win+Tab
        Command::Window(target) => {
            if IsWindow(target) == 0 {
                return;
            }
            if GetForegroundWindow() == target && IsIconic(target) == 0 {
                ShowWindow(target, SW_MINIMIZE);
            } else {
                if IsIconic(target) != 0 {
                    ShowWindow(target, SW_RESTORE);
                }
                SetForegroundWindow(target);
            }
        }
        Command::Exit => {
            DestroyWindow(hwnd);
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

unsafe fn fill(hdc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    if !brush.is_null() {
        FillRect(hdc, rect, brush);
        DeleteObject(brush);
    }
}

unsafe fn label(hdc: HDC, rect: RECT, text: &str, color: u32) {
    label_with_alignment(hdc, rect, text, color, DT_CENTER);
}

unsafe fn label_with_alignment(hdc: HDC, rect: RECT, text: &str, color: u32, align: u32) {
    let utf16 = wide(text);
    let mut bounds = rect;
    SetTextColor(hdc, color);
    DrawTextW(
        hdc,
        utf16.as_ptr(),
        -1,
        &mut bounds,
        DT_SINGLELINE | align | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

unsafe fn draw_content(hdc: HDC, state: &AppState, width: i32, height: i32) {
    let background = rgb(23, 27, 36);
    let inactive = rgb(42, 48, 62);
    let hover = rgb(54, 64, 85);
    let active = rgb(59, 82, 128);
    let text = rgb(230, 234, 246);
    let muted = rgb(170, 178, 192);
    fill(hdc, &RECT { left: 0, top: 0, right: width, bottom: height }, background);
    SetBkMode(hdc, TRANSPARENT as i32);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let old_font = SelectObject(hdc, font);

    let layout = model::layout(width, height, state.dpi, state.windows.len());
    let foreground = GetForegroundWindow();
    for button in &layout.buttons {
        let (name, selected) = match button.action {
            Action::Start => ("Start".to_owned(), false),
            Action::Search => ("Search".to_owned(), false),
            Action::Window(index) => {
                let entry = &state.windows[index];
                (entry.title.clone(), foreground == entry.hwnd)
            }
            Action::AllWindows => (format!("+{}", layout.hidden_windows), false),
            Action::Exit => ("Exit".to_owned(), false),
        };
        let bounds = RECT {
            left: button.rect.left,
            top: button.rect.top,
            right: button.rect.right,
            bottom: button.rect.bottom,
        };
        fill(
            hdc,
            &bounds,
            if selected { active } else if state.hovered == Some(button.action) { hover } else { inactive },
        );
        // Icons are cached and copied once at enumeration; WM_PAINT never
        // sends a synchronous message to another application's window.
        let mut padded = RECT { left: bounds.left + 5, right: bounds.right - 5, ..bounds };
        if let Action::Window(index) = button.action {
            if let Some(icon) = state.windows[index].icon.as_ref() {
                let icon_size = model::scale(20, state.dpi);
                let icon_x = bounds.left + model::scale(9, state.dpi);
                let icon_y = bounds.top + (bounds.bottom - bounds.top - icon_size) / 2;
                DrawIconEx(hdc, icon_x, icon_y, icon.0, icon_size, icon_size, 0, null_mut(), DI_NORMAL);
                padded.left = icon_x + icon_size + model::scale(7, state.dpi);
                label_with_alignment(hdc, padded, &name, text, DT_LEFT);
                continue;
            }
        }
        label(hdc, padded, &name, text);
    }

    let mut time: SYSTEMTIME = zeroed();
    GetLocalTime(&mut time);
    let clock = format!("{:02}:{:02}", time.wHour, time.wMinute);
    label(
        hdc,
        RECT {
            left: layout.clock.left,
            right: layout.clock.right,
            top: layout.clock.top,
            bottom: layout.clock.bottom,
        },
        &clock,
        muted,
    );
    SelectObject(hdc, old_font);
}

unsafe fn paint(hwnd: HWND, state: &AppState) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_null() {
        return;
    }
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    let width = client.right.max(1);
    let height = client.bottom.max(1);

    // Double-buffer Win32 GDI for flicker-free event-driven rendering.
    // Unlike an animation loop, this runs only after invalidation.
    let offscreen = CreateCompatibleDC(hdc);
    if !offscreen.is_null() {
        let bitmap = CreateCompatibleBitmap(hdc, width, height);
        if !bitmap.is_null() {
            let old = SelectObject(offscreen, bitmap);
            draw_content(offscreen, state, width, height);
            BitBlt(hdc, 0, 0, width, height, offscreen, 0, 0, SRCCOPY);
            SelectObject(offscreen, old);
            DeleteObject(bitmap);
        } else {
            draw_content(hdc, state, width, height);
        }
        DeleteDC(offscreen);
    } else {
        draw_content(hdc, state, width, height);
    }
    EndPaint(hwnd, &ps);
}

/// Recompute hovered button only on pointer events. Does not start a timer.
unsafe fn update_hover(hwnd: HWND, state: &mut AppState, x: i32, y: i32) {
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    let layout = model::layout(client.right, client.bottom, state.dpi, state.windows.len());
    let next = model::hit_test(&layout, x, y);
    if state.hovered != next {
        state.hovered = next;
        InvalidateRect(hwnd, null(), 0);
    }
}

// SAFETY: AppState is a stable Box for the entire message pump lifetime. All
// window notifications and enumeration run synchronously on the same UI thread.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return 1;
    }
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if (*state_ptr).shell_message != 0 && message == (*state_ptr).shell_message {
        refresh_windows(hwnd, &mut *state_ptr);
        return 0;
    }

    match message {
        WM_PAINT => {
            paint(hwnd, &*state_ptr);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let x = (lparam as u32 as u16 as i16) as i32;
            let y = ((lparam as u32 >> 16) as u16 as i16) as i32;
            if !(*state_ptr).tracking_mouse {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                (*state_ptr).tracking_mouse = TrackMouseEvent(&mut tracking) != 0;
            }
            update_hover(hwnd, &mut *state_ptr, x, y);
            0
        }
        WM_MOUSELEAVE => {
            (*state_ptr).tracking_mouse = false;
            if (*state_ptr).hovered.take().is_some() {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            let x = (lparam as u32 as u16 as i16) as i32;
            let y = ((lparam as u32 >> 16) as u16 as i16) as i32;
            let command = resolve_click(hwnd, &*state_ptr, x, y);
            if let Some(command) = command {
                execute_command(hwnd, command);
            }
            0
        }
        WM_TIMER if wparam == FALLBACK_TIMER => {
            refresh_windows(hwnd, &mut *state_ptr);
            0
        }
        WM_TIMER if wparam == CLOCK_TIMER => {
            KillTimer(hwnd, CLOCK_TIMER);
            schedule_clock(hwnd);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED => {
            let dpi = GetDpiForWindow(hwnd).max(96);
            (*state_ptr).dpi = dpi;
            position_window(hwnd, dpi);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_TIMECHANGE => {
            KillTimer(hwnd, CLOCK_TIMER);
            schedule_clock(hwnd);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_KEYDOWN if wparam == usize::from(VK_ESCAPE) => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, CLOCK_TIMER);
            KillTimer(hwnd, FALLBACK_TIMER);
            if (*state_ptr).shell_message != 0 {
                DeregisterShellHookWindow(hwnd);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
