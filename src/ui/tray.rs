use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::Sender;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tracing::info;
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

use crate::app::events::{AppEvent, UserAction};

pub fn spawn(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-tray".into())
        .spawn(move || {
            if let Err(err) = run_tray(event_tx, running) {
                tracing::error!("tray thread failed: {err}");
            }
        })?;
    Ok(())
}

fn run_tray(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let icon = Icon::from_rgba(vec![0xFF, 0xC0, 0x40, 0xFF].repeat(16 * 16), 16, 16)?;

    let exit_item = MenuItem::new("Exit", true, None);
    let exit_id = exit_item.id().clone();
    let menu = Menu::new();
    menu.append(&exit_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::new("Hold Win+Ctrl to dictate", false, None))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Squeak — voice dictation")
        .with_icon(icon)
        .build()?;

    info!("Tray icon ready");

    while running.load(Ordering::Relaxed) {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == exit_id {
                let _ = event_tx.send(AppEvent::UserAction(UserAction::Exit));
            }
        }
        if let Ok(_event) = TrayIconEvent::receiver().try_recv() {
            // Left click — no-op in v1
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }

    Ok(())
}
