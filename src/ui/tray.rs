use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Sender;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tracing::info;
use tray_icon::{Icon, TrayIconBuilder};

use crate::app::{AppEvent, UserAction};

pub fn spawn(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-tray".into())
        .spawn(move || {
            if let Err(err) = run_tray(event_tx, running) {
                tracing::error!("tray thread failed: {err}");
                eprintln!("Squeak tray failed: {err}");
            }
        })?;
    Ok(())
}

fn run_tray(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let icon = Icon::from_rgba(vec![0xFF, 0xC0, 0x40, 0xFF].repeat(16 * 16), 16, 16)?;

    let exit_item = MenuItem::new("Exit", true, None);
    let exit_id = exit_item.id().clone();
    let menu = Menu::new();
    menu.append(&exit_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::new("Hold Win+Ctrl to dictate", false, None))?;

    let event_tx_menu = event_tx.clone();

    // tray-icon requires a Win32 message pump on this thread *before* Shell_NotifyIcon
    // reliably registers (see tauri-apps/tray-icon issue #90).
    let mut tray = None;
    let mut initialized = false;

    unsafe {
        let mut msg = MSG::default();
        while running.load(Ordering::Relaxed) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if !initialized {
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu))
                        .with_tooltip("Squeak — voice dictation")
                        .with_icon(icon)
                        .build()?,
                );

                MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                    if exit_id == event.id() {
                        let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::Exit));
                    }
                }));

                info!("Tray icon ready");
                eprintln!(
                    "Squeak tray icon active — check the ^ overflow area in the taskbar if you do not see it."
                );
                initialized = true;
            }

            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    drop(tray);
    Ok(())
}
