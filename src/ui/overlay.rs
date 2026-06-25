//! Primary-monitor recording/processing pill overlay (U9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use tracing::info;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn, CreateSolidBrush,
    DeleteDC, DeleteObject, EndPaint, FillRect, GetDC, GetMonitorInfoW, InvalidateRect,
    MonitorFromPoint, ReleaseDC, RoundRect, SelectClipRgn, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBRUSH, HRGN, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, PeekMessageW, RegisterClassW, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, UpdateLayeredWindow, GWLP_USERDATA, HWND_TOPMOST, MSG, PM_REMOVE,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, ULW_ALPHA,
    WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::AppState;
use crate::audio::AudioLevelMeter;
use crate::overlay_grow::{
    animated_scale, display_width, recording_width_fraction, scaled_dimension, OVERLAY_HEIGHT,
    OVERLAY_WIDTH, PILL_CORNER, PILL_MIN_WIDTH,
};
use crate::overlay_raster::{apply_coverage_mask, pill_coverage_mask};
use crate::ui_visual::{
    lerp_rgb, phase_display_scale, phase_uses_grow_animation, ptt_hold_fraction, scale_rgb,
    ui_phase, UiPhase,
};

const LAYERED_BLEND: BLENDFUNCTION = BLENDFUNCTION {
    BlendOp: AC_SRC_OVER as u8,
    BlendFlags: 0,
    SourceConstantAlpha: 255,
    AlphaFormat: AC_SRC_ALPHA as u8,
};

const PILL_INSET: i32 = 5;
/// Gap above the taskbar within the monitor work area (Wispr Flow–style bottom dock).
const OVERLAY_BOTTOM_MARGIN: i32 = 16;
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
    /// Bottom edge of the pill sits this many px above `rcWork.bottom`.
    anchor_bottom_y: i32,
    grow_start_ms: u64,
    asr_ready: bool,
    asr_ready_ms: Option<u64>,
    scale_from: f32,
    scale_to: f32,
    scale_start_ms: u64,
    current_width: i32,
    current_height: i32,
    /// Cached brushes/regions to reduce GDI overhead in the render loop.
    cached_shell_brushes: Vec<HBRUSH>,
    cached_gloss_brushes: Vec<HBRUSH>,
    cached_shade_brush: HBRUSH,
    cached_volume_bar_brush: HBRUSH,
    cached_inner_clip: HRGN,
    /// Cached plasma brushes — recreated at most every 100 ms (10 fps visual).
    cached_plasma_brushes: Vec<HBRUSH>,
    last_plasma_update: u64,
    /// Cached per-pixel coverage mask (keyed by width/height).
    cached_coverage: Option<Vec<u8>>,
    cached_coverage_size: (i32, i32),
    /// Reusable layered bitmap (DIB section + DC) — avoids allocation on every frame.
    cached_layer: Option<LayeredBitmap>,
    cached_layer_size: (i32, i32),
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

    let (anchor_center_x, anchor_bottom_y) = primary_monitor_anchor();
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        PCWSTR(class_name.as_ptr()),
        PCWSTR::null(),
        WS_POPUP,
        anchor_center_x - PILL_MIN_WIDTH / 2,
        anchor_bottom_y - OVERLAY_HEIGHT,
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
        anchor_bottom_y,
        grow_start_ms: 0,
        asr_ready: false,
        asr_ready_ms: None,
        scale_from: 1.0,
        scale_to: 1.0,
        scale_start_ms: 0,
        current_width: PILL_MIN_WIDTH,
        current_height: OVERLAY_HEIGHT,
        cached_shell_brushes: Vec::new(),
        cached_gloss_brushes: Vec::new(),
        cached_shade_brush: HBRUSH::default(),
        cached_volume_bar_brush: HBRUSH::default(),
        cached_inner_clip: HRGN::default(),
        cached_plasma_brushes: Vec::new(),
        last_plasma_update: 0,
        cached_coverage: None,
        cached_coverage_size: (0, 0),
        cached_layer: None,
        cached_layer_size: (0, 0),
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    let _ = ShowWindow(hwnd, SW_HIDE);
    if SetTimer(hwnd, ANIM_TIMER_ID, ANIM_MS, None) == 0 {
        return Err("overlay SetTimer failed".into());
    }

    info!("Recording overlay ready");
    eprintln!("When you dictate, a pill indicator appears at the bottom center of your screen.");

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

fn pill_corner_for_size(width: i32, height: i32) -> i32 {
    PILL_CORNER.min(width.max(1)).min(height.max(1))
}

fn base_pill_width(state: &OverlayWindowState, now_ms: u64) -> i32 {
    match state.phase {
        UiPhase::Hidden => PILL_MIN_WIDTH,
        UiPhase::Processing => OVERLAY_WIDTH,
        UiPhase::Armed | UiPhase::RecordingPtt | UiPhase::RecordingHandsFree => {
            let allow_full_grow = !matches!(state.phase, UiPhase::Armed);
            let frac = recording_width_fraction(
                state.grow_start_ms,
                now_ms,
                state.asr_ready,
                state.asr_ready_ms,
                allow_full_grow,
            );
            display_width(frac)
        }
    }
}

fn current_display_scale(state: &OverlayWindowState, now_ms: u64) -> f32 {
    animated_scale(
        state.scale_from,
        state.scale_to,
        state.scale_start_ms,
        now_ms,
    )
}

fn scale_animation_active(state: &OverlayWindowState, now_ms: u64) -> bool {
    state.scale_start_ms != 0 && current_display_scale(state, now_ms) != state.scale_to
}

fn begin_scale_transition(state: &mut OverlayWindowState, target: f32, now_ms: u64) {
    let current = current_display_scale(state, now_ms);
    if (current - target).abs() < f32::EPSILON {
        state.scale_from = target;
        state.scale_to = target;
        state.scale_start_ms = 0;
        return;
    }
    state.scale_from = current;
    state.scale_to = target;
    state.scale_start_ms = now_ms;
}

unsafe fn update_pill_geometry(hwnd: HWND, state: &mut OverlayWindowState) {
    if state.phase == UiPhase::Hidden {
        return;
    }

    let now = GetTickCount64();
    let scale = current_display_scale(state, now);
    let base_w = base_pill_width(state, now);
    let width = scaled_dimension(base_w, scale).max(scaled_dimension(PILL_MIN_WIDTH, scale));
    let height = scaled_dimension(OVERLAY_HEIGHT, scale).max(8);

    if width != state.current_width || height != state.current_height {
        state.current_width = width;
        state.current_height = height;

        // Invalidate cached region on resize
        if !state.cached_inner_clip.is_invalid() {
            let _ = DeleteObject(state.cached_inner_clip);
            state.cached_inner_clip = HRGN::default();
        }

        let x = state.anchor_center_x - width / 2;
        let y = state.anchor_bottom_y - height;
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = InvalidateRect(hwnd, None, false);
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
    begin_scale_transition(state, phase_display_scale(phase), GetTickCount64());

    match phase {
        UiPhase::Hidden => {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        UiPhase::Armed
        | UiPhase::RecordingPtt
        | UiPhase::RecordingHandsFree
        | UiPhase::Processing => {
            if prev == UiPhase::Hidden && phase != UiPhase::Processing {
                reset_recording_grow(state);
            }
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
        let bottom_y = work.bottom - OVERLAY_BOTTOM_MARGIN;
        return (center_x, bottom_y);
    }

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    (screen_w / 2, screen_h - OVERLAY_BOTTOM_MARGIN)
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
        let mut state = Box::from_raw(ptr);
        for brush in state.cached_shell_brushes.drain(..) {
            let _ = DeleteObject(brush);
        }
        for brush in state.cached_gloss_brushes.drain(..) {
            let _ = DeleteObject(brush);
        }
        for brush in state.cached_plasma_brushes.drain(..) {
            if !brush.is_invalid() {
                let _ = DeleteObject(brush);
            }
        }
        if !state.cached_shade_brush.is_invalid() {
            let _ = DeleteObject(state.cached_shade_brush);
        }
        if !state.cached_volume_bar_brush.is_invalid() {
            let _ = DeleteObject(state.cached_volume_bar_brush);
        }
        if !state.cached_inner_clip.is_invalid() {
            let _ = DeleteObject(state.cached_inner_clip);
        }
        // Drop cached layer (will run its Drop impl)
        state.cached_layer = None;
        state.cached_coverage = None;
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
            let _ = BeginPaint(hwnd, &mut ps);
            present_layered_overlay(hwnd);
            let _ = EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ANIM_TIMER_ID => {
            if let Some(state) = overlay_state_mut(hwnd) {
                let now = GetTickCount64();
                if phase_uses_grow_animation(state.phase) || scale_animation_active(state, now) {
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

struct LayeredBitmap {
    screen_dc: windows::Win32::Graphics::Gdi::HDC,
    mem_dc: windows::Win32::Graphics::Gdi::HDC,
    old_bitmap: windows::Win32::Graphics::Gdi::HGDIOBJ,
    dib: windows::Win32::Graphics::Gdi::HBITMAP,
    bits: *mut std::ffi::c_void,
    byte_len: usize,
}

impl Drop for LayeredBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(self.dib);
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(HWND::default(), self.screen_dc);
        }
    }
}

unsafe fn present_layered_overlay(hwnd: HWND) {
    let (width, height) = match overlay_state_mut(hwnd) {
        Some(state) if state.phase != UiPhase::Hidden => {
            (state.current_width, state.current_height)
        }
        _ => return,
    };
    if width <= 0 || height <= 0 {
        return;
    }

    let state = overlay_state_mut(hwnd).unwrap();

    // Reuse or create the layered bitmap (DIB section + DC) to avoid per-frame allocation.
    let need_new_layer = state.cached_layer.is_none()
        || state.cached_layer_size != (width, height);
    let layer = if need_new_layer {
        // Clean up old layer
        state.cached_layer = None;

        let screen_dc = GetDC(HWND::default());
        if screen_dc.is_invalid() {
            return;
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_invalid() {
            let _ = ReleaseDC(HWND::default(), screen_dc);
            return;
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(HWND::default(), screen_dc);
                return;
            }
        };

        let byte_len = (width * height * 4) as usize;
        let layer = LayeredBitmap {
            screen_dc,
            mem_dc,
            old_bitmap: SelectObject(mem_dc, dib),
            dib,
            bits,
            byte_len,
        };
        state.cached_layer = Some(layer);
        state.cached_layer_size = (width, height);
        state.cached_layer.as_mut().unwrap()
    } else {
        state.cached_layer.as_mut().unwrap()
    };

    if !layer.bits.is_null() {
        std::ptr::write_bytes(layer.bits.cast::<u8>(), 0, layer.byte_len);
    }

    paint_overlay(hwnd, layer.mem_dc, width, height);

    if state.cached_coverage.is_none() || state.cached_coverage_size != (width, height) {
        state.cached_coverage = Some(pill_coverage_mask(width, height));
        state.cached_coverage_size = (width, height);
    }

    if !layer.bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(layer.bits.cast::<u8>(), layer.byte_len);
        apply_coverage_mask(
            pixels,
            state.cached_coverage.as_ref().unwrap(),
            width,
            height,
        );
    }

    let mut window_rect = RECT::default();
    if GetWindowRect(hwnd, &mut window_rect).is_err() {
        return;
    }

    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&POINT {
            x: window_rect.left,
            y: window_rect.top,
        }),
        Some(&SIZE {
            cx: width,
            cy: height,
        }),
        layer.mem_dc,
        Some(&POINT { x: 0, y: 0 }),
        COLORREF(0),
        Some(&LAYERED_BLEND),
        ULW_ALPHA,
    );
}

fn pill_inset_for_height(height: i32) -> i32 {
    let inset = (PILL_INSET as f32 * (height as f32 / OVERLAY_HEIGHT as f32)).round() as i32;
    inset.clamp(2, height / 3)
}

fn inner_pill_geometry(width: i32, height: i32) -> (RECT, i32) {
    let inset = pill_inset_for_height(height);
    let inner = RECT {
        left: inset,
        top: inset,
        right: width - inset,
        bottom: height - inset,
    };
    let inner_r = (pill_corner_for_size(width, height) - inset).max(4);
    (inner, inner_r)
}

unsafe fn paint_overlay(
    hwnd: HWND,
    hdc: windows::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
) {
    use windows::Win32::Foundation::RECT;

    let Some(state) = overlay_state_mut(hwnd) else {
        return;
    };
    if state.phase == UiPhase::Hidden {
        return;
    }

    let now_ms = GetTickCount64();
    let tick_s = now_ms as f64 / 1000.0;
    let (inner, inner_r) = inner_pill_geometry(width, height);

    // Convex dome: lighter crown, darker base. Silhouette AA is applied after paint.
    const SHELL_BANDS: i32 = 10;
    if state.cached_shell_brushes.is_empty() {
        for band in 0..SHELL_BANDS {
            let t = band as f32 / (SHELL_BANDS - 1) as f32;
            let (r, g, b) = lerp_rgb((8, 0, 14), (72, 18, 88), 1.0 - t);
            state
                .cached_shell_brushes
                .push(CreateSolidBrush(colorref(r, g, b)));
        }
    }

    let band_h = (height + SHELL_BANDS - 1) / SHELL_BANDS;
    for (band, &brush) in state.cached_shell_brushes.iter().enumerate() {
        let y0 = band as i32 * band_h;
        let y1 = (y0 + band_h).min(height);
        let strip = RECT {
            left: 0,
            top: y0,
            right: width,
            bottom: y1,
        };
        let _ = FillRect(hdc, &strip, brush);
    }

    if state.cached_inner_clip.is_invalid() {
        state.cached_inner_clip = CreateRoundRectRgn(
            inner.left,
            inner.top,
            inner.right,
            inner.bottom,
            inner_r,
            inner_r,
        );
    }
    let _ = SelectClipRgn(hdc, state.cached_inner_clip);

    // Plasma brushes: update at most every 100 ms (10 fps) instead of every frame.
    const PLASMA_UPDATE_MS: u64 = 100;
    let need_plasma_update = state.cached_plasma_brushes.is_empty()
        || now_ms.saturating_sub(state.last_plasma_update) >= PLASMA_UPDATE_MS;
    if need_plasma_update {
        // Clean up old brushes
        for &brush in &state.cached_plasma_brushes {
            if !brush.is_invalid() {
                let _ = DeleteObject(brush);
            }
        }
        state.cached_plasma_brushes.clear();

        let inner_w = inner.right - inner.left;
        let strip_w = (inner_w + PLASMA_STRIPS - 1) / PLASMA_STRIPS;
        for i in 0..PLASMA_STRIPS {
            let x_norm = (i as f32 + 0.5) / PLASMA_STRIPS as f32;
            let (r, g, b) = plasma_rgb(x_norm, 0.5, tick_s as f32, state.phase);
            state
                .cached_plasma_brushes
                .push(CreateSolidBrush(colorref(r, g, b)));
        }
        state.last_plasma_update = now_ms;
    }

    let inner_w = inner.right - inner.left;
    let strip_w = (inner_w + PLASMA_STRIPS - 1) / PLASMA_STRIPS;
    for i in 0..PLASMA_STRIPS {
        let x0 = inner.left + i * strip_w;
        let x1 = (x0 + strip_w).min(inner.right);
        let brush = state.cached_plasma_brushes[i as usize];
        let strip = RECT {
            left: x0,
            top: inner.top,
            right: x1,
            bottom: inner.bottom,
        };
        let _ = FillRect(hdc, &strip, brush);
    }

    // Subtle top gloss (3D dome highlight) + bottom ambient shade.
    paint_gloss(hdc, inner, state);

    let shade_h = 3;
    if state.cached_shade_brush.is_invalid() {
        state.cached_shade_brush = CreateSolidBrush(colorref(12, 0, 18));
    }
    let _ = FillRect(
        hdc,
        &RECT {
            left: inner.left,
            top: inner.bottom - shade_h,
            right: inner.right,
            bottom: inner.bottom,
        },
        state.cached_shade_brush,
    );

    if state.phase == UiPhase::Armed {
        paint_ptt_hold_bar(hdc, inner, state.grow_start_ms);
    } else {
        paint_volume_bars(hdc, inner, state);
    }

    let _ = SelectClipRgn(hdc, HRGN::default());
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
    let v = (x * 4.8 + tt).sin() + (y * 3.6 - tt * 0.7).sin() + ((x + y) * 3.0 + tt * 0.45).sin();
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

/// Soft crown highlight — tapered width follows the pill curve.
unsafe fn paint_gloss(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    inner: windows::Win32::Foundation::RECT,
    state: &mut OverlayWindowState,
) {
    use windows::Win32::Foundation::RECT;

    const GLOSS_ROWS: i32 = 4;
    if state.cached_gloss_brushes.is_empty() {
        for row in 0..GLOSS_ROWS {
            let t = row as f32 / (GLOSS_ROWS - 1) as f32;
            let (r, g, b) = lerp_rgb((168, 98, 178), (105, 42, 118), t);
            state
                .cached_gloss_brushes
                .push(CreateSolidBrush(colorref(r, g, b)));
        }
    }

    let row_h = 1.max((inner.bottom - inner.top) / 12);
    let inner_w = inner.right - inner.left;

    for (row, &brush) in state.cached_gloss_brushes.iter().enumerate() {
        let row = row as i32;
        // Narrower at the top row to suggest a curved reflective surface.
        let side_inset = (4 + row * 5).min(inner_w / 3);
        let band = RECT {
            left: inner.left + side_inset,
            top: inner.top + row * row_h,
            right: inner.right - side_inset,
            bottom: inner.top + (row + 1) * row_h,
        };
        let _ = FillRect(hdc, &band, brush);
    }
}

unsafe fn paint_volume_bars(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    inner: windows::Win32::Foundation::RECT,
    state: &mut OverlayWindowState,
) {
    if state.cached_volume_bar_brush.is_invalid() {
        state.cached_volume_bar_brush = CreateSolidBrush(colorref(210, 120, 185));
    }
    let old = SelectObject(hdc, state.cached_volume_bar_brush);

    let bar_w = 3;
    let gap = 5;
    let bar_count = 5;
    let total_w = bar_w * bar_count + gap * (bar_count - 1);
    let start_x = inner.left + (inner.right - inner.left - total_w) / 2;
    let center_y = (inner.top + inner.bottom) / 2;
    let max_half = ((inner.bottom - inner.top) / 2 - 2).max(3);

    let levels = if matches!(
        state.phase,
        UiPhase::RecordingPtt | UiPhase::RecordingHandsFree
    ) {
        state.meter.bar_levels()
    } else {
        // Processing: gentle decay of last captured levels.
        state.meter.bar_levels().map(|l| (l * 0.55).max(0.08))
    };

    for (i, level) in levels.iter().enumerate() {
        let height = (2 + (level * max_half as f32 * 2.0) as i32).clamp(2, max_half * 2);
        let x = start_x + i as i32 * (bar_w + gap);
        let top = center_y - height / 2;
        let bottom = center_y + height / 2;
        let _ = RoundRect(hdc, x, top, x + bar_w, bottom, 1, 1);
    }

    let _ = SelectObject(hdc, old);
}

fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}
