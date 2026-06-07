//! Primary-monitor recording/processing pill overlay (U9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tracing::info;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, GetMonitorInfoW, HRGN, InvalidateRect, MonitorFromPoint, RoundRect, SelectClipRgn,
    SelectObject, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT, PS_SOLID,
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

/// Capsule pill — 25% smaller than the original 168×44 indicator.
const OVERLAY_WIDTH: i32 = 126;
const OVERLAY_HEIGHT: i32 = 33;
/// True pill ends: semicircle radius = half height.
const PILL_RADIUS: i32 = OVERLAY_HEIGHT / 2;
const OVERLAY_TOP_MARGIN: i32 = 16;
const ANIM_TIMER_ID: usize = 1;
const ANIM_MS: u32 = 60;
const PLASMA_STRIPS: i32 = 42;

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
            let _ = InvalidateRect(hwnd, None, false);
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
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_overlay(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};

    let mode = overlay_mode(hwnd);
    if mode == OverlayMode::Hidden {
        return;
    }

    let tick_s = GetTickCount64() as f64 / 1000.0;

    let pill = RECT {
        left: 0,
        top: 0,
        right: OVERLAY_WIDTH,
        bottom: OVERLAY_HEIGHT,
    };

    let clip = CreateRoundRectRgn(
        pill.left,
        pill.top,
        pill.right,
        pill.bottom,
        PILL_RADIUS,
        PILL_RADIUS,
    );
    let _ = SelectClipRgn(hdc, clip);

    let strip_w = (OVERLAY_WIDTH + PLASMA_STRIPS - 1) / PLASMA_STRIPS;
    for i in 0..PLASMA_STRIPS {
        let x0 = i * strip_w;
        let x1 = (x0 + strip_w).min(OVERLAY_WIDTH);
        let x_norm = (i as f32 + 0.5) / PLASMA_STRIPS as f32;
        let (r, g, b) = plasma_rgb(x_norm, 0.5, tick_s as f32, mode);
        let brush = CreateSolidBrush(colorref(r, g, b));
        let strip = RECT {
            left: x0,
            top: 0,
            right: x1,
            bottom: OVERLAY_HEIGHT,
        };
        let _ = FillRect(hdc, &strip, brush);
        let _ = DeleteObject(brush);
    }

    let _ = SelectClipRgn(hdc, HRGN::default());
    let _ = DeleteObject(clip);

    // Subtle outline so the pill reads on light and dark wallpapers.
    let border = CreatePen(PS_SOLID, 1, colorref(55, 8, 72));
    let old_pen = SelectObject(hdc, border);
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let _ = RoundRect(
        hdc,
        pill.left,
        pill.top,
        pill.right,
        pill.bottom,
        PILL_RADIUS,
        PILL_RADIUS,
    );
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(border);

    paint_waveform_bars(hdc, mode, tick_s);
}

/// Flowing plasma palette inspired by ASCII Plasma (purple → magenta → pink).
fn plasma_rgb(x: f32, y: f32, t: f32, mode: OverlayMode) -> (u8, u8, u8) {
    let speed = if mode == OverlayMode::Processing {
        0.45
    } else {
        1.0
    };
    let tt = t * speed;
    let v = (x * 5.2 + tt).sin()
        + (y * 4.0 - tt * 0.85).sin()
        + ((x + y) * 3.6 + tt * 0.55).sin()
        + ((x * 2.0 - y * 1.5 + tt * 0.35).sin() * 0.6);
    let v = ((v + 3.2) / 6.4).clamp(0.0, 1.0);

    if v < 0.35 {
        lerp_rgb((28, 0, 42), (95, 0, 88), v / 0.35)
    } else if v < 0.68 {
        lerp_rgb((95, 0, 88), (196, 32, 150), (v - 0.35) / 0.33)
    } else {
        lerp_rgb((196, 32, 150), (255, 128, 210), (v - 0.68) / 0.32)
    }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

unsafe fn paint_waveform_bars(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    mode: OverlayMode,
    tick_s: f64,
) {
    let bar_brush = CreateSolidBrush(colorref(255, 210, 245));
    let old = SelectObject(hdc, bar_brush);

    let bar_w = 4;
    let gap = 6;
    let total_w = bar_w * 5 + gap * 4;
    let start_x = (OVERLAY_WIDTH - total_w) / 2;
    let center_y = OVERLAY_HEIGHT / 2;

    for i in 0..5 {
        let phase = tick_s / 0.18 + i as f64 * 0.9;
        let height = if mode == OverlayMode::Processing {
            5 + ((phase * 2.0).sin().abs() * 4.0) as i32
        } else {
            4 + (phase.sin().abs() * 10.0) as i32
        };
        let x = start_x + i * (bar_w + gap);
        let top = center_y - height / 2;
        let bottom = center_y + height / 2;
        let _ = RoundRect(hdc, x, top, x + bar_w, bottom, 2, 2);
    }

    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(bar_brush);
}

fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}
