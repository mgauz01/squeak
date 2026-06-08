use std::cell::RefCell;
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};

use crossbeam_channel::Sender;
use tracing::{error, info};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_SHIFT, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL,
    VK_LWIN, VK_RCONTROL, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    PostThreadMessageW, RegisterClassW, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_HOTKEY,
    WM_KEYDOWN, WM_QUIT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::app::{AppEvent, RecordingMode, UserAction};
use crate::hotkeys::gestures::{GestureFsm, GestureOutput, Key};

const PASTE_LAST_ID: i32 = 1;
const HOTKEY_VK_Z: u32 = 0x5A;
const GESTURE_TIMER_ID: usize = 1;
const GESTURE_TICK_MS: u32 = 50;

static EVENT_TX: OnceLock<Sender<AppEvent>> = OnceLock::new();
static HOTKEY_THREAD: OnceLock<u32> = OnceLock::new();

thread_local! {
    static HOOK_HANDLE: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
    static GESTURE_FSM: RefCell<GestureFsm> = RefCell::new(GestureFsm::default());
}

pub fn spawn(event_tx: Sender<AppEvent>) -> JoinHandle<()> {
    let _ = EVENT_TX.set(event_tx);
    thread::Builder::new()
        .name("squeak-hotkeys".into())
        .spawn(|| {
            if let Err(err) = unsafe { run_message_loop() } {
                error!("hotkey thread failed: {err}");
                eprintln!("Squeak hotkeys failed: {err}");
            }
        })
        .expect("spawn hotkey thread")
}

pub fn shutdown() {
    if let Some(tid) = HOTKEY_THREAD.get() {
        unsafe {
            let _ = PostThreadMessageW(*tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn run_message_loop() -> Result<(), Box<dyn std::error::Error>> {
    let _ = HOTKEY_THREAD.set(GetCurrentThreadId());

    let hwnd = create_message_window()?;
    if SetTimer(hwnd, GESTURE_TIMER_ID, GESTURE_TICK_MS, None) == 0 {
        return Err("SetTimer failed".into());
    }
    RegisterHotKey(hwnd, PASTE_LAST_ID, MOD_SHIFT | MOD_ALT, HOTKEY_VK_Z)?;

    let instance: HINSTANCE = GetModuleHandleW(None)?.into();
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), instance, 0)?;
    HOOK_HANDLE.with(|h| *h.borrow_mut() = Some(hook));

    info!("Win+Ctrl hook and Shift+Alt+Z paste-last registered");
    eprintln!("Hotkeys active — hold Win+Ctrl for at least 300 ms to start dictating.");

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        if msg.message == WM_HOTKEY && msg.wParam.0 == PASTE_LAST_ID as usize {
            send_event(AppEvent::UserAction(UserAction::PasteLast));
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    let _ = UnregisterHotKey(hwnd, PASTE_LAST_ID);
    let _ = UnhookWindowsHookEx(hook);
    Ok(())
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if let Some(key) = virtual_key_to_gesture_key(kb.vkCode) {
            let pressed = wparam.0 as u32 == WM_KEYDOWN as u32;
            let now = GetTickCount64();
            GESTURE_FSM.with(|fsm| {
                let mut fsm = fsm.borrow_mut();
                emit_gesture(fsm.on_tick(now));
                emit_gesture(fsm.on_key(key, pressed, now));
            });
        }
    }

    HOOK_HANDLE.with(|h| {
        let hook = h.borrow().unwrap_or_default();
        CallNextHookEx(hook, code, wparam, lparam)
    })
}

fn emit_gesture(output: GestureOutput) {
    if let Some(event) = gesture_to_event(output) {
        send_event(event);
    }
}

fn gesture_to_event(output: GestureOutput) -> Option<AppEvent> {
    match output {
        GestureOutput::None => None,
        GestureOutput::ArmMicrophone => Some(AppEvent::ArmRecording),
        GestureOutput::DisarmMicrophone => Some(AppEvent::DisarmRecording),
        GestureOutput::StartPushToTalk => Some(AppEvent::StartRecording {
            mode: RecordingMode::PushToTalk,
        }),
        GestureOutput::StopPushToTalk => Some(AppEvent::StopRecording),
        GestureOutput::StartHandsFree => Some(AppEvent::StartRecording {
            mode: RecordingMode::HandsFree,
        }),
        GestureOutput::StopHandsFree => Some(AppEvent::StopRecording),
    }
}

fn virtual_key_to_gesture_key(vk: u32) -> Option<Key> {
    match VIRTUAL_KEY(vk as u16) {
        VK_LWIN | VK_RWIN => Some(Key::Win),
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(Key::Ctrl),
        _ => None,
    }
}

fn send_event(event: AppEvent) {
    if let Some(tx) = EVENT_TX.get() {
        let _ = tx.send(event);
    }
}

unsafe extern "system" fn message_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TIMER && wparam.0 == GESTURE_TIMER_ID {
        let now = GetTickCount64();
        GESTURE_FSM.with(|fsm| {
            emit_gesture(fsm.borrow_mut().on_tick(now));
        });
        return LRESULT(0);
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn create_message_window() -> Result<HWND, Box<dyn std::error::Error>> {
    let class_name: Vec<u16> = "SqueakHotkeyWindow\0".encode_utf16().collect();
    let wc = WNDCLASSW {
        lpfnWndProc: Some(message_window_proc),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    RegisterClassW(&wc);

    let hwnd = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        PCWSTR(class_name.as_ptr()),
        PCWSTR(class_name.as_ptr()),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        HWND::default(),
        None,
        None,
        None,
    )?;

    Ok(hwnd)
}
