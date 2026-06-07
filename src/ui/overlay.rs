//! Primary-monitor recording/processing pill overlay (U9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tracing::info;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect,
    MonitorFromPoint, RoundRect, SelectObject, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, GetWindowLongPtrW,
    PeekMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, MSG, PM_REMOVE,
    SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW,
    WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::app::AppState;

const OVERLAY_WIDTH: i32 = 168;
const OVERLAY_HEIGHT: i32 = 44;
const OVERLAY_TOP_MARGIN: i32 = 16;
const ANIM_TIMER_ID: usize = 1;
const ANIM_MS: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Hidden = 0,
    Recording = 1,
    Processing = 2,
}

pub fn spawn(
    cmd_rx: Receiver<OverlayMode>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-overlay".into())
        .spawn(move || {
            if let Err(err) = unsafe { run_overlay(cmd_rx, running) } {
                tracing::error!("overlay thread failed: {err}");
                eprintln!("Squeak overlay failed: {err}");
            }
        })?;
    Ok(())
}

pub fn sync(tx: &Sender<OverlayMode>, state: AppState) {
    let mode = match state {
        AppState::RecordingPtt | AppState::RecordingHandsFree => OverlayMode::Recording,
        AppState::Processing | AppState::Injecting => OverlayMode::Processing,
        _ => OverlayMode::Hidden,
    };
    let _ = tx.send(mode);
}

unsafe fn run_overlay(
    cmd_rx: Receiver<OverlayMode>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let class_name: Vec<u16> = "SqueakOverlay\0".encode_utf16().collect();
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_wnd_proc),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    RegisterClassW(&wc);

    let (x, y) = primary_monitor_top_center();
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        PCWSTR(class_name.as_ptr()),
        PCWSTR::null(),
        WS_POPUP,
        x,
        y,
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
        HWND::default(),
        None,
        None,
        None,
    )?;

    set_overlay_mode(hwnd, OverlayMode::Hidden);
    let _ = ShowWindow(hwnd, SW_HIDE);
    if SetTimer(hwnd, ANIM_TIMER_ID, ANIM_MS, None) == 0 {
        return Err("overlay SetTimer failed".into());
    }

    info!("Recording overlay ready");
    eprintln!("When you dictate, a pill indicator appears at the top center of your screen.");

    let mut msg = MSG::default();
    while running.load(Ordering::Relaxed) {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
            if msg.message == WM_DESTROY {
                return Ok(());
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        match cmd_rx.try_recv() {
            Ok(mode) => apply_overlay_mode(hwnd, mode),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        thread::sleep(std::time::Duration::from_millis(10));
    }

    Ok(())
}

unsafe fn apply_overlay_mode(hwnd: HWND, mode: OverlayMode) {
    set_overlay_mode(hwnd, mode);
    match mode {
        OverlayMode::Hidden => {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        OverlayMode::Recording | OverlayMode::Processing => {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

unsafe fn primary_monitor_top_center() -> (i32, i32) {
    let pt = POINT { x: 0, y: 0 };
    let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).0 != 0 {
        let work = info.rcWork;
        let x = work.left + (work.right - work.left - OVERLAY_WIDTH) / 2;
        let y = work.top + OVERLAY_TOP_MARGIN;
        return (x, y);
    }

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    ((screen_w - OVERLAY_WIDTH) / 2, OVERLAY_TOP_MARGIN)
}

unsafe fn set_overlay_mode(hwnd: HWND, mode: OverlayMode) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, mode as isize);
}

unsafe fn overlay_mode(hwnd: HWND) -> OverlayMode {
    match GetWindowLongPtrW(hwnd, GWLP_USERDATA) {
        1 => OverlayMode::Recording,
        2 => OverlayMode::Processing,
        _ => OverlayMode::Hidden,
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                paint_overlay(hwnd, hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ANIM_TIMER_ID => {
            if overlay_mode(hwnd) != OverlayMode::Hidden {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_overlay(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
    use windows::Win32::Foundation::RECT;

    let mode = overlay_mode(hwnd);
    if mode == OverlayMode::Hidden {
        return;
    }

    let rect = RECT {
        left: 0,
        top: 0,
        right: OVERLAY_WIDTH,
        bottom: OVERLAY_HEIGHT,
    };

    let bg = CreateSolidBrush(colorref(16, 16, 16));
    let old = SelectObject(hdc, bg);
    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 22, 22);
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(bg);

    let tick = GetTickCount64();
    let bar_brush = CreateSolidBrush(colorref(245, 245, 245));
    let old = SelectObject(hdc, bar_brush);

    let bar_w = 6;
    let gap = 8;
    let total_w = bar_w * 5 + gap * 4;
    let start_x = (OVERLAY_WIDTH - total_w) / 2;
    let center_y = OVERLAY_HEIGHT / 2;

    for i in 0..5 {
        let phase = tick as f64 / 180.0 + i as f64 * 0.9;
        let height = if mode == OverlayMode::Processing {
            8 + ((phase * 2.0).sin().abs() * 6.0) as i32
        } else {
            6 + (phase.sin().abs() * 14.0) as i32
        };
        let x = start_x + i * (bar_w + gap);
        let top = center_y - height / 2;
        let bottom = center_y + height / 2;
        let _ = RoundRect(hdc, x, top, x + bar_w, bottom, 3, 3);
    }

    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(bar_brush);
}

fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}
