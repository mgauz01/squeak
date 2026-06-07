//! Primary-monitor recording/processing pill overlay (U9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tracing::info;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, GetMonitorInfoW, HRGN, InvalidateRect, MonitorFromPoint, RoundRect, SelectClipRgn,
    SelectObject, SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT, PS_SOLID,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, GetWindowLongPtrW,
    PeekMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, MSG, PM_REMOVE,
    SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW,
    WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::AppState;
use crate::audio::AudioLevelMeter;

/// Capsule pill — 25% smaller than the original 168×44 indicator.
const OVERLAY_WIDTH: i32 = 126;
const OVERLAY_HEIGHT: i32 = 33;
/// True pill ends: semicircle radius = half height.
const PILL_RADIUS: i32 = OVERLAY_HEIGHT / 2;
const PILL_INSET: i32 = 5;
const OVERLAY_TOP_MARGIN: i32 = 16;
const ANIM_TIMER_ID: usize = 1;
const ANIM_MS: u32 = 50;
const PLASMA_STRIPS: i32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Hidden = 0,
    Recording = 1,
    Processing = 2,
}

struct OverlayWindowState {
    mode: OverlayMode,
    meter: Arc<AudioLevelMeter>,
}

pub fn spawn(
    cmd_rx: Receiver<OverlayMode>,
    running: Arc<AtomicBool>,
    meter: Arc<AudioLevelMeter>,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-overlay".into())
        .spawn(move || {
            if let Err(err) = unsafe { run_overlay(cmd_rx, running, meter) } {
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
    meter: Arc<AudioLevelMeter>,
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

    let state = Box::new(OverlayWindowState {
        mode: OverlayMode::Hidden,
        meter,
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    apply_pill_window_shape(hwnd);

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
                free_overlay_state(hwnd);
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

    free_overlay_state(hwnd);
    Ok(())
}

unsafe fn apply_pill_window_shape(hwnd: HWND) {
    let rgn = CreateRoundRectRgn(
        0,
        0,
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
        PILL_RADIUS,
        PILL_RADIUS,
    );
    let _ = SetWindowRgn(hwnd, rgn, BOOL::from(true));
}

unsafe fn apply_overlay_mode(hwnd: HWND, mode: OverlayMode) {
    if let Some(state) = overlay_state_mut(hwnd) {
        state.mode = mode;
    }
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

unsafe fn overlay_state_mut(hwnd: HWND) -> Option<&'static mut OverlayWindowState> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindowState;
    if ptr.is_null() {
        None
    } else {
        Some(&mut *ptr)
    }
}

unsafe fn free_overlay_state(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindowState;
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
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
            if let Some(state) = overlay_state_mut(hwnd) {
                if state.mode != OverlayMode::Hidden {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            free_overlay_state(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn outer_pill_rect() -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: OVERLAY_WIDTH,
        bottom: OVERLAY_HEIGHT,
    }
}

fn inner_pill_rect() -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: PILL_INSET,
        top: PILL_INSET,
        right: OVERLAY_WIDTH - PILL_INSET,
        bottom: OVERLAY_HEIGHT - PILL_INSET,
    }
}

fn inner_pill_radius() -> i32 {
    (PILL_RADIUS - PILL_INSET).max(4)
}

unsafe fn paint_overlay(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};

    let Some(state) = overlay_state_mut(hwnd) else {
        return;
    };
    if state.mode == OverlayMode::Hidden {
        return;
    }

    let tick_s = GetTickCount64() as f64 / 1000.0;
    let outer = outer_pill_rect();
    let inner = inner_pill_rect();
    let inner_r = inner_pill_radius();

    // Opaque pill shell (no black corners — window region is already pill-shaped).
    let shell = CreateSolidBrush(colorref(10, 0, 16));
    let old = SelectObject(hdc, shell);
    let _ = RoundRect(
        hdc,
        outer.left,
        outer.top,
        outer.right,
        outer.bottom,
        PILL_RADIUS,
        PILL_RADIUS,
    );
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(shell);

    let inner_clip = CreateRoundRectRgn(
        inner.left,
        inner.top,
        inner.right,
        inner.bottom,
        inner_r,
        inner_r,
    );
    let _ = SelectClipRgn(hdc, inner_clip);

    let inner_w = inner.right - inner.left;
    let strip_w = (inner_w + PLASMA_STRIPS - 1) / PLASMA_STRIPS;
    for i in 0..PLASMA_STRIPS {
        let x0 = inner.left + i * strip_w;
        let x1 = (x0 + strip_w).min(inner.right);
        let x_norm = (i as f32 + 0.5) / PLASMA_STRIPS as f32;
        let (r, g, b) = plasma_rgb(x_norm, 0.5, tick_s as f32, state.mode);
        let brush = CreateSolidBrush(colorref(r, g, b));
        let strip = RECT {
            left: x0,
            top: inner.top,
            right: x1,
            bottom: inner.bottom,
        };
        let _ = FillRect(hdc, &strip, brush);
        let _ = DeleteObject(brush);
    }

    paint_volume_bars(hdc, inner, state.mode, &state.meter);

    let _ = SelectClipRgn(hdc, HRGN::default());
    let _ = DeleteObject(inner_clip);

    let border = CreatePen(PS_SOLID, 1, colorref(38, 4, 52));
    let old_pen = SelectObject(hdc, border);
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let _ = RoundRect(
        hdc,
        outer.left,
        outer.top,
        outer.right,
        outer.bottom,
        PILL_RADIUS,
        PILL_RADIUS,
    );
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(border);
}

/// Darker, inset plasma palette (purple → magenta → muted pink).
fn plasma_rgb(x: f32, y: f32, t: f32, mode: OverlayMode) -> (u8, u8, u8) {
    let speed = if mode == OverlayMode::Processing {
        0.35
    } else {
        0.75
    };
    let tt = t * speed;
    let v = (x * 4.8 + tt).sin()
        + (y * 3.6 - tt * 0.7).sin()
        + ((x + y) * 3.0 + tt * 0.45).sin();
    let v = ((v + 2.4) / 4.8).clamp(0.0, 1.0);
    let v = v * 0.72 + 0.08;

    if v < 0.4 {
        lerp_rgb((14, 0, 22), (48, 0, 58), v / 0.4)
    } else if v < 0.72 {
        lerp_rgb((48, 0, 58), (98, 10, 82), (v - 0.4) / 0.32)
    } else {
        lerp_rgb((98, 10, 82), (150, 40, 118), (v - 0.72) / 0.28)
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

unsafe fn paint_volume_bars(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    inner: windows::Win32::Foundation::RECT,
    mode: OverlayMode,
    meter: &AudioLevelMeter,
) {
    let bar_brush = CreateSolidBrush(colorref(210, 120, 185));
    let old = SelectObject(hdc, bar_brush);

    let bar_w = 3;
    let gap = 5;
    let bar_count = 5;
    let total_w = bar_w * bar_count + gap * (bar_count - 1);
    let start_x = inner.left + (inner.right - inner.left - total_w) / 2;
    let center_y = (inner.top + inner.bottom) / 2;
    let max_half = ((inner.bottom - inner.top) / 2 - 2).max(3);

    let levels = if mode == OverlayMode::Recording {
        meter.bar_levels()
    } else {
        // Processing: gentle decay of last captured levels.
        meter
            .bar_levels()
            .map(|l| (l * 0.55).max(0.08))
    };

    for (i, level) in levels.iter().enumerate() {
        let height = (2 + (level * max_half as f32 * 2.0) as i32).clamp(2, max_half * 2);
        let x = start_x + i as i32 * (bar_w + gap);
        let top = center_y - height / 2;
        let bottom = center_y + height / 2;
        let _ = RoundRect(hdc, x, top, x + bar_w, bottom, 1, 1);
    }

    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(bar_brush);
}

fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}
