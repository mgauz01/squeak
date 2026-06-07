use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Sender;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tracing::info;
use tray_icon::{Icon, TrayIconBuilder};

use crate::app::{AppEvent, UserAction};
use crate::config::{AsrModelId, ModelTier};

use windows::Win32::UI::WindowsAndMessaging::MSG;

pub fn spawn(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
    initial_model: AsrModelId,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-tray".into())
        .spawn(move || {
            if let Err(err) = run_tray(event_tx, running, initial_model) {
                tracing::error!("tray thread failed: {err}");
                eprintln!("Squeak tray failed: {err}");
            }
        })?;
    Ok(())
}

fn build_tray_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    let size = 16u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = 7.5f32;
    let outer = 7.0f32;
    let inner = 4.5f32;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if dist <= outer {
                rgba[i] = 0xFF;
                rgba[i + 1] = 0xA0;
                rgba[i + 2] = 0x20;
                rgba[i + 3] = 0xFF;
            }
            if dist <= inner {
                rgba[i] = 0x20;
                rgba[i + 1] = 0x20;
                rgba[i + 2] = 0x20;
                rgba[i + 3] = 0xFF;
            }
        }
    }

    Ok(Icon::from_rgba(rgba, size, size)?)
}

fn append_model_item(
    menu: &Submenu,
    model: AsrModelId,
    active: AsrModelId,
    model_ids: &mut Vec<(muda::MenuId, AsrModelId)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let item = CheckMenuItem::new(
        model.menu_label(),
        true,
        model == active,
        None,
    );
    model_ids.push((item.id().clone(), model));
    menu.append(&item)?;
    Ok(())
}

fn run_tray(
    event_tx: Sender<AppEvent>,
    running: Arc<AtomicBool>,
    initial_model: AsrModelId,
) -> Result<(), Box<dyn std::error::Error>> {
    let icon = build_tray_icon()?;

    let exit_item = MenuItem::new("Exit", true, None);
    let exit_id = exit_item.id().clone();

    let model_menu = Submenu::new("Speech model", true);
    let mut model_ids = Vec::new();

    let moonshine_menu = Submenu::new("Moonshine", true);
    for model in AsrModelId::MOONSHINE_ALL {
        append_model_item(&moonshine_menu, model, initial_model, &mut model_ids)?;
    }
    model_menu.append(&moonshine_menu)?;

    #[cfg(feature = "parakeet")]
    append_model_item(
        &model_menu,
        AsrModelId::Parakeet,
        initial_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "cohere")]
    append_model_item(
        &model_menu,
        AsrModelId::Cohere,
        initial_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "canary")]
    append_model_item(
        &model_menu,
        AsrModelId::Canary,
        initial_model,
        &mut model_ids,
    )?;

    let menu = Menu::new();
    menu.append(&model_menu)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::new("Hold Win+Ctrl to dictate", false, None))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&exit_item)?;

    let event_tx_menu = event_tx.clone();

    unsafe {
        let mut msg = MSG::default();

        dispatch_pending_messages(&mut msg);

        let _tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Squeak — voice dictation (tray: Speech model for accuracy)")
            .with_icon(icon)
            .build()?;

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if exit_id == event.id() {
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::Exit));
                return;
            }
            for (id, model) in &model_ids {
                if *id == event.id() {
                    let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::SetAsrModel(
                        model.config_key(),
                    )));
                    eprintln!("Switching speech model to {}…", model.tray_summary());
                    return;
                }
            }
        }));

        info!("Tray icon ready");
        eprintln!(
            "Squeak is in the system tray (orange ring). Dictation shows a pill at the top of the screen."
        );
        eprintln!(
            "Speech model: {} (default Small). Use tray → Speech model to change.",
            initial_model.tray_summary()
        );

        while running.load(Ordering::Relaxed) {
            dispatch_pending_messages(&mut msg);
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    Ok(())
}

/// Windows requires a Win32 message pump on the tray thread or the icon never appears.
unsafe fn dispatch_pending_messages(msg: &mut MSG) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, PM_REMOVE,
    };

    while PeekMessageW(msg, None, 0, 0, PM_REMOVE).into() {
        let _ = TranslateMessage(msg);
        DispatchMessageW(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moonshine_tier_maps_to_asr_model() {
        for tier in ModelTier::ALL {
            let model = AsrModelId::moonshine(tier);
            assert_eq!(model.moonshine_tier(), Some(tier));
        }
    }
}
