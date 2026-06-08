//! Primary-monitor recording/processing pill overlay (U9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use tracing::info;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateRoundRectRgn,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, GetMonitorInfoW, HRGN,
    InvalidateRect, MonitorFromPoint, RoundRect, SelectClipRgn, SelectObject, SetWindowRgn,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, GetWindowLongPtrW,
    PeekMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, GWLP_USERDATA, HWND_TOPMOST, MSG, PM_REMOVE, SM_CXSCREEN, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_DESTROY, WM_ERASEBKGND,
    WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::AppState;
use crate::audio::AudioLevelMeter;
use crate::overlay_grow::{
    display_width, recording_width_fraction, OVERLAY_HEIGHT, OVERLAY_WIDTH, PILL_CORNER,
    PILL_MIN_WIDTH,
};
use crate::ui_visual::{phase_uses_grow_animation, ptt_hold_fraction, ui_phase, UiPhase};

const PILL_INSET: i32 = 5;
const OVERLAY_TOP_MARGIN: i32 = 16;
const ANIM_TIMER_ID: usize = 1;
const ANIM_MS: u32 = 50;
const PLASMA_STRIPS: i32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCommand {
    SetPhase(UiPhase),
    AsrReady,
}

struct OverlayWindowState {
    phase: UiPhase,
    meter: Arc<AudioLevelMeter>,
    anchor_center_x: i32,
    anchor_y: i32,
    grow_start_ms: u64,
    asr_ready: bool,
    asr_ready_ms: Option<u64>,
    current_width: i32,
}

pub fn spawn(
    cmd_rx: Receiver<OverlayCommand>,
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

pub fn sync(tx: &Sender<OverlayCommand>, app: AppState, mic_armed: bool) {
    set_phase(tx, ui_phase(app, mic_armed));
}

/// Push overlay phase directly (e.g. show pill as soon as Win+Ctrl is pressed).
pub fn set_phase(tx: &Sender<OverlayCommand>, phase: UiPhase) {
    let _ = tx.send(OverlayCommand::SetPhase(phase));
}

/// ASR finished loading — complete the horizontal grow-in animation.
pub fn signal_asr_ready(tx: &Sender<OverlayCommand>) {
    let _ = tx.send(OverlayCommand::AsrReady);
}

unsafe fn run_overlay(
    cmd_rx: Receiver<OverlayCommand>,
    running: Arc<AtomicBool>,
    meter: Arc<AudioLevelMeter>,
) -> Result<(), Box<dyn std::error::Error>> {
    let class_name: Vec<u16> = "SqueakOverlay\0".encode_utf16().collect();
    let wc = WNDCLASSW {
        lpfnWndProc: Some(overlay_wnd_proc),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    RegisterClassW(&wc);

    let (anchor_center_x, anchor_y) = primary_monitor_anchor();
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        PCWSTR(class_name.as_ptr()),
        PCWSTR::null(),
        WS_POPUP,
        anchor_center_x - PILL_MIN_WIDTH / 2,
        anchor_y,
        PILL_MIN_WIDTH,
        OVERLAY_HEIGHT,
        HWND::default(),
        None,
        None,
        None,
    )?;

    let state = Box::new(OverlayWindowState {
        phase: UiPhase::Hidden,
        meter,
        anchor_center_x,
        anchor_y,
        grow_start_ms: 0,
        asr_ready: false,
        asr_ready_ms: None,
        current_width: PILL_MIN_WIDTH,
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    apply_pill_window_shape(hwnd, PILL_MIN_WIDTH);

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

        match cmd_rx.recv_timeout(std::time::Duration::from_millis(16)) {
            Ok(OverlayCommand::SetPhase(phase)) => apply_overlay_phase(hwnd, phase),
            Ok(OverlayCommand::AsrReady) => apply_asr_ready(hwnd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    free_overlay_state(hwnd);
    Ok(())
}

fn pill_corner_for_width(width: i32) -> i32 {
    PILL_CORNER.min(width.max(1))
}

unsafe fn apply_pill_window_shape(hwnd: HWND, width: i32) {
    let corner = pill_corner_for_width(width);
    let rgn = CreateRoundRectRgn(0, 0, width, OVERLAY_HEIGHT, corner, corner);
    let _ = SetWindowRgn(hwnd, rgn, BOOL::from(true));
}

unsafe fn update_pill_geometry(hwnd: HWND, state: &mut OverlayWindowState) {
    let width = match state.phase {
        UiPhase::Hidden => return,
        UiPhase::Processing => OVERLAY_WIDTH,
        UiPhase::Armed | UiPhase::RecordingPtt | UiPhase::RecordingHandsFree => {
            let now = GetTickCount64();
            let frac = recording_width_fraction(
                state.grow_start_ms,
                now,
                state.asr_ready,
                state.asr_ready_ms,
            );
            display_width(frac)
        }
    };

    if width != state.current_width {
        state.current_width = width;
        let x = state.anchor_center_x - width / 2;
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            state.anchor_y,
            width,
            OVERLAY_HEIGHT,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        apply_pill_window_shape(hwnd, width);
    }
}

unsafe fn reset_recording_grow(state: &mut OverlayWindowState) {
    state.grow_start_ms = GetTickCount64();
    state.asr_ready = false;
    state.asr_ready_ms = None;
    state.current_width = PILL_MIN_WIDTH;
}

unsafe fn apply_overlay_phase(hwnd: HWND, phase: UiPhase) {
    let Some(state) = overlay_state_mut(hwnd) else {
        return;
    };
    if state.phase == phase {
        return;
    }
    let prev = state.phase;
    state.phase = phase;

    match phase {
        UiPhase::Hidden => {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        UiPhase::Armed | UiPhase::RecordingPtt | UiPhase::RecordingHandsFree => {
            if prev == UiPhase::Hidden {
                reset_recording_grow(state);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            update_pill_geometry(hwnd, state);
            let _ = InvalidateRect(hwnd, None, false);
        }
        UiPhase::Processing => {
            state.current_width = OVERLAY_WIDTH;
            let _ = ShowWindow(hwnd, SW_SHOW);
            update_pill_geometry(hwnd, state);
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

unsafe fn apply_asr_ready(hwnd: HWND) {
    let Some(state) = overlay_state_mut(hwnd) else {
        return;
    };
    if !phase_uses_grow_animation(state.phase) || state.asr_ready {
        return;
    }
    state.asr_ready = true;
    state.asr_ready_ms = Some(GetTickCount64());
    update_pill_geometry(hwnd, state);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn primary_monitor_anchor() -> (i32, i32) {
    let pt = POINT { x: 0, y: 0 };
    let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).0 != 0 {
        let work = info.rcWork;
        let center_x = work.left + (work.right - work.left) / 2;
        let y = work.top + OVERLAY_TOP_MARGIN;
        return (center_x, y);
    }

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    (screen_w / 2, OVERLAY_TOP_MARGIN)
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
                let width = overlay_state_mut(hwnd)
                    .map(|s| s.current_width)
                    .unwrap_or(PILL_MIN_WIDTH);
                let mem_dc = CreateCompatibleDC(hdc);
                if !mem_dc.is_invalid() {
                    let bitmap = CreateCompatibleBitmap(hdc, width, OVERLAY_HEIGHT);
                    if !bitmap.is_invalid() {
                        let old_bitmap = SelectObject(mem_dc, bitmap);
                        paint_overlay(hwnd, mem_dc, width);
                        let _ = BitBlt(
                            hdc,
                            0,
                            0,
                            width,
                            OVERLAY_HEIGHT,
                            mem_dc,
                            0,
                            0,
                            SRCCOPY,
                        );
                        let _ = SelectObject(mem_dc, old_bitmap);
                        let _ = DeleteObject(bitmap);
                    }
                    let _ = DeleteDC(mem_dc);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ANIM_TIMER_ID => {
            if let Some(state) = overlay_state_mut(hwnd) {
                if phase_uses_grow_animation(state.phase) {
                    update_pill_geometry(hwnd, state);
                }
                if state.phase != UiPhase::Hidden {
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

fn outer_pill_rect(width: i32) -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: OVERLAY_HEIGHT,
    }
}

fn inner_pill_rect(width: i32) -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: PILL_INSET,
        top: PILL_INSET,
        right: width - PILL_INSET,
        bottom: OVERLAY_HEIGHT - PILL_INSET,
    }
}

fn inner_pill_radius(width: i32) -> i32 {
    (pill_corner_for_width(width) - PILL_INSET).max(4)
}

unsafe fn paint_overlay(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC, width: i32) {
    use windows::Win32::Foundation::RECT;

    let Some(state) = overlay_state_mut(hwnd) else {
        return;
    };
    if state.phase == UiPhase::Hidden {
        return;
    }

    let tick_s = GetTickCount64() as f64 / 1000.0;
    let outer = outer_pill_rect(width);
    let inner = inner_pill_rect(width);
    let outer_corner = pill_corner_for_width(width);
    let inner_r = inner_pill_radius(width);

    let outer_clip = CreateRoundRectRgn(
        outer.left,
        outer.top,
        outer.right,
        outer.bottom,
        outer_corner,
        outer_corner,
    );
    let _ = SelectClipRgn(hdc, outer_clip);

    // Convex dome: lighter crown, darker base (no outline stroke).
    const SHELL_BANDS: i32 = 10;
    let band_h = (OVERLAY_HEIGHT + SHELL_BANDS - 1) / SHELL_BANDS;
    for band in 0..SHELL_BANDS {
        let y0 = outer.top + band * band_h;
        let y1 = (y0 + band_h).min(outer.bottom);
        let t = band as f32 / (SHELL_BANDS - 1) as f32;
        let (r, g, b) = lerp_rgb((8, 0, 14), (72, 18, 88), 1.0 - t);
        let brush = CreateSolidBrush(colorref(r, g, b));
        let strip = RECT {
            left: outer.left,
            top: y0,
            right: outer.right,
            bottom: y1,
        };
        let _ = FillRect(hdc, &strip, brush);
        let _ = DeleteObject(brush);
    }

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
        let (r, g, b) = plasma_rgb(x_norm, 0.5, tick_s as f32, state.phase);
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

    // Subtle top gloss (3D dome highlight) + bottom ambient shade.
    paint_gloss(hdc, inner);

    let shade_h = 3;
    let shade = CreateSolidBrush(colorref(12, 0, 18));
    let _ = FillRect(
        hdc,
        &RECT {
            left: inner.left,
            top: inner.bottom - shade_h,
            right: inner.right,
            bottom: inner.bottom,
        },
        shade,
    );
    let _ = DeleteObject(shade);

    if state.phase == UiPhase::Armed {
        paint_ptt_hold_bar(hdc, inner, state.grow_start_ms);
    } else {
        paint_volume_bars(hdc, inner, state.phase, &state.meter);
    }

    let _ = SelectClipRgn(hdc, HRGN::default());
    let _ = DeleteObject(inner_clip);
    let _ = DeleteObject(outer_clip);
}

/// Darker, inset plasma palette (purple → magenta → muted pink).
fn plasma_rgb(x: f32, y: f32, t: f32, phase: UiPhase) -> (u8, u8, u8) {
    let speed = match phase {
        UiPhase::Processing => 0.35,
        UiPhase::Armed => 0.45,
        _ => 0.75,
    };
    let brightness = match phase {
        UiPhase::Armed => 0.55,
        UiPhase::RecordingHandsFree => 1.1,
        UiPhase::Processing => 0.88,
        _ => 1.0,
    };
    let tt = t * speed;
    let v = (x * 4.8 + tt).sin()
        + (y * 3.6 - tt * 0.7).sin()
        + ((x + y) * 3.0 + tt * 0.45).sin();
    let v = ((v + 2.4) / 4.8).clamp(0.0, 1.0);
    let v = v * 0.72 + 0.08;

    let base = if v < 0.4 {
        lerp_rgb((14, 0, 22), (48, 0, 58), v / 0.4)
    } else if v < 0.72 {
        lerp_rgb((48, 0, 58), (98, 10, 82), (v - 0.4) / 0.32)
    } else {
        lerp_rgb((98, 10, 82), (150, 40, 118), ((v - 0.72) / 0.28).min(1.0))
    };
    scale_rgb(base, brightness)
}

fn scale_rgb((r, g, b): (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    (
        ((r as f32) * factor).min(255.0) as u8,
        ((g as f32) * factor).min(255.0) as u8,
        ((b as f32) * factor).min(255.0) as u8,
    )
}

/// Bottom-edge fill showing Win+Ctrl hold progress toward PTT (armed only).
unsafe fn paint_ptt_hold_bar(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    inner: windows::Win32::Foundation::RECT,
    arm_start_ms: u64,
) {
    use windows::Win32::Foundation::RECT;

    let frac = ptt_hold_fraction(arm_start_ms, GetTickCount64());
    let inner_w = inner.right - inner.left;
    let fill_w = ((inner_w as f32 - 4.0) * frac).max(0.0) as i32;
    if fill_w <= 0 {
        return;
    }
    let bar_h = 3;
    let (r, g, b) = lerp_rgb((168, 98, 178), (210, 120, 185), frac);
    let brush = CreateSolidBrush(colorref(r, g, b));
    let bar = RECT {
        left: inner.left + 2,
        top: inner.bottom - bar_h - 1,
        right: inner.left + 2 + fill_w,
        bottom: inner.bottom - 1,
    };
    let _ = FillRect(hdc, &bar, brush);
    let _ = DeleteObject(brush);
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

/// Soft crown highlight — tapered width follows the pill curve.
unsafe fn paint_gloss(hdc: windows::Win32::Graphics::Gdi::HDC, inner: windows::Win32::Foundation::RECT) {
    use windows::Win32::Foundation::RECT;

    const GLOSS_ROWS: i32 = 4;
    let row_h = 1.max((inner.bottom - inner.top) / 12);
    let inner_w = inner.right - inner.left;

    for row in 0..GLOSS_ROWS {
        let t = row as f32 / (GLOSS_ROWS - 1) as f32;
        let (r, g, b) = lerp_rgb((168, 98, 178), (105, 42, 118), t);
        // Narrower at the top row to suggest a curved reflective surface.
        let side_inset = (4 + row * 5).min(inner_w / 3);
        let brush = CreateSolidBrush(colorref(r, g, b));
        let band = RECT {
            left: inner.left + side_inset,
            top: inner.top + row * row_h,
            right: inner.right - side_inset,
            bottom: inner.top + (row + 1) * row_h,
        };
        let _ = FillRect(hdc, &band, brush);
        let _ = DeleteObject(brush);
    }
}

unsafe fn paint_volume_bars(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    inner: windows::Win32::Foundation::RECT,
    phase: UiPhase,
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

    let levels = if matches!(phase, UiPhase::RecordingPtt | UiPhase::RecordingHandsFree) {
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
