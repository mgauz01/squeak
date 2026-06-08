use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tracing::info;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::app::{AppEvent, UserAction};
use crate::asr::recommended_thread_count;
use crate::config::{AsrModelId, GrammarModelId};
use crate::ui_visual::{tray_icon_rgba, tray_icon_state, TrayIconState, UiPhase};

use windows::Win32::UI::WindowsAndMessaging::MSG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    SetPhase(UiPhase),
}

/// Initial tray menu state mirrored from `config.toml`.
#[derive(Debug, Clone, Copy)]
pub struct TrayInit {
    pub asr_model: AsrModelId,
    pub grammar_enabled: bool,
    pub grammar_model: GrammarModelId,
    pub directml: bool,
    pub xnnpack: bool,
    pub asr_threads: usize,
    pub autostart: bool,
}

pub fn spawn(
    event_tx: Sender<AppEvent>,
    status_rx: Receiver<TrayCommand>,
    running: Arc<AtomicBool>,
    init: TrayInit,
) -> Result<(), Box<dyn std::error::Error>> {
    thread::Builder::new()
        .name("squeak-tray".into())
        .spawn(move || {
            if let Err(err) = run_tray(event_tx, status_rx, running, init) {
                tracing::error!("tray thread failed: {err}");
                eprintln!("Squeak tray failed: {err}");
            }
        })?;
    Ok(())
}

pub fn set_phase(tx: &Sender<TrayCommand>, phase: UiPhase) {
    let _ = tx.send(TrayCommand::SetPhase(phase));
}

fn build_tray_icon(state: TrayIconState) -> Result<Icon, Box<dyn std::error::Error>> {
    let size = 16u32;
    let rgba = tray_icon_rgba(state, size);
    Ok(Icon::from_rgba(rgba, size, size)?)
}

fn append_model_item(
    menu: &Submenu,
    model: AsrModelId,
    active: AsrModelId,
    model_ids: &mut Vec<(muda::MenuId, AsrModelId)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let item = CheckMenuItem::new(model.menu_label(), true, model == active, None);
    model_ids.push((item.id().clone(), model));
    menu.append(&item)?;
    Ok(())
}

fn append_grammar_item(
    menu: &Submenu,
    label: &str,
    key: &str,
    checked: bool,
    grammar_ids: &mut Vec<(muda::MenuId, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let item = CheckMenuItem::new(label, true, checked, None);
    grammar_ids.push((item.id().clone(), key.to_string()));
    menu.append(&item)?;
    Ok(())
}

fn append_thread_item(
    menu: &Submenu,
    label: &str,
    threads: usize,
    active: usize,
    thread_ids: &mut Vec<(muda::MenuId, usize)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let item = CheckMenuItem::new(label, true, threads == active, None);
    thread_ids.push((item.id().clone(), threads));
    menu.append(&item)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TrayToggleState {
    directml: bool,
    xnnpack: bool,
    autostart: bool,
}

fn run_tray(
    event_tx: Sender<AppEvent>,
    status_rx: Receiver<TrayCommand>,
    running: Arc<AtomicBool>,
    init: TrayInit,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_icon = TrayIconState::Idle;
    let icon = build_tray_icon(current_icon)?;

    let exit_item = MenuItem::new("Exit", true, None);
    let exit_id = exit_item.id().clone();

    let model_menu = Submenu::new("Speech model", true);
    let mut model_ids = Vec::new();

    let moonshine_menu = Submenu::new("Moonshine", true);
    for model in AsrModelId::MOONSHINE_ALL {
        append_model_item(&moonshine_menu, model, init.asr_model, &mut model_ids)?;
    }
    model_menu.append(&moonshine_menu)?;

    #[cfg(feature = "parakeet")]
    append_model_item(
        &model_menu,
        AsrModelId::Parakeet,
        init.asr_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "cohere")]
    append_model_item(
        &model_menu,
        AsrModelId::Cohere,
        init.asr_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "canary")]
    append_model_item(
        &model_menu,
        AsrModelId::Canary,
        init.asr_model,
        &mut model_ids,
    )?;

    let threads_menu = Submenu::new("ASR CPU threads", true);
    let mut thread_ids = Vec::new();
    let auto_threads = recommended_thread_count();
    append_thread_item(
        &threads_menu,
        &format!("Auto ({auto_threads} threads)"),
        0,
        init.asr_threads,
        &mut thread_ids,
    )?;
    for n in [4_usize, 8, 16] {
        append_thread_item(
            &threads_menu,
            &format!("{n} threads"),
            n,
            init.asr_threads,
            &mut thread_ids,
        )?;
    }

    let directml_item = CheckMenuItem::new(
        "Use DirectML (GPU, Moonshine only)",
        true,
        init.directml,
        None,
    );
    let directml_id = directml_item.id().clone();

    let xnnpack_item = CheckMenuItem::new(
        "Use XNNPACK CPU kernels (experimental)",
        true,
        init.xnnpack,
        None,
    );
    let xnnpack_id = xnnpack_item.id().clone();

    let autostart_item = CheckMenuItem::new("Start with Windows", true, init.autostart, None);
    let autostart_id = autostart_item.id().clone();

    let open_config_item = MenuItem::new("Open config.toml…", true, None);
    let open_config_id = open_config_item.id().clone();

    let toggle_state = RefCell::new(TrayToggleState {
        directml: init.directml,
        xnnpack: init.xnnpack,
        autostart: init.autostart,
    });

    #[cfg(not(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama")))]
    let _ = (init.grammar_enabled, init.grammar_model);

    #[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
    let mut grammar_ids: Vec<(muda::MenuId, String)> = Vec::new();
    #[cfg(not(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama")))]
    let grammar_ids: Vec<(muda::MenuId, String)> = Vec::new();
    #[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
    let grammar_menu = {
        let grammar_menu = Submenu::new("Grammar correction (experimental)", true);
        append_grammar_item(
            &grammar_menu,
            "Off",
            "off",
            !init.grammar_enabled,
            &mut grammar_ids,
        )?;
        for model in GrammarModelId::all_models() {
            append_grammar_item(
                &grammar_menu,
                model.menu_label(),
                model.config_key(),
                init.grammar_enabled && init.grammar_model == model,
                &mut grammar_ids,
            )?;
        }
        grammar_menu
    };

    let menu = Menu::new();
    menu.append(&model_menu)?;
    #[cfg(any(feature = "gec-tiny", feature = "gec-coedit", feature = "gec-llama"))]
    menu.append(&grammar_menu)?;
    menu.append(&threads_menu)?;
    menu.append(&directml_item)?;
    menu.append(&xnnpack_item)?;
    menu.append(&autostart_item)?;
    menu.append(&open_config_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::new("Hold Win+Ctrl to dictate", false, None))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&exit_item)?;

    let event_tx_menu = event_tx.clone();

    unsafe {
        let mut msg = MSG::default();

        dispatch_pending_messages(&mut msg);

        let tray: TrayIcon = TrayIconBuilder::new()
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
            for (id, key) in &grammar_ids {
                if *id == event.id() {
                    let _ = event_tx_menu.send(AppEvent::UserAction(
                        UserAction::SetGrammarProfile(key.clone()),
                    ));
                    eprintln!("Switching grammar profile to {key}…");
                    return;
                }
            }
            for (id, threads) in &thread_ids {
                if *id == event.id() {
                    let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::SetAsrThreads(
                        *threads,
                    )));
                    eprintln!("Setting ASR CPU threads to {}…", threads_label(*threads));
                    return;
                }
            }
            if directml_id == event.id() {
                let mut toggles = toggle_state.borrow_mut();
                toggles.directml = !toggles.directml;
                let _ = directml_item.set_checked(toggles.directml);
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::ToggleDirectMl(
                    toggles.directml,
                )));
                eprintln!(
                    "{} DirectML…",
                    if toggles.directml { "Enabling" } else { "Disabling" }
                );
                return;
            }
            if xnnpack_id == event.id() {
                let mut toggles = toggle_state.borrow_mut();
                toggles.xnnpack = !toggles.xnnpack;
                let _ = xnnpack_item.set_checked(toggles.xnnpack);
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::ToggleXnnpack(
                    toggles.xnnpack,
                )));
                eprintln!(
                    "{} XNNPACK…",
                    if toggles.xnnpack { "Enabling" } else { "Disabling" }
                );
                return;
            }
            if autostart_id == event.id() {
                let mut toggles = toggle_state.borrow_mut();
                toggles.autostart = !toggles.autostart;
                let _ = autostart_item.set_checked(toggles.autostart);
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::ToggleAutostart(
                    toggles.autostart,
                )));
                eprintln!(
                    "{} start with Windows…",
                    if toggles.autostart { "Enabling" } else { "Disabling" }
                );
                return;
            }
            if open_config_id == event.id() {
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::OpenSettings));
                return;
            }
        }));

        info!("Tray icon ready");
        eprintln!(
            "Squeak is in the system tray (purple pill icon). Dictation shows a pill at the bottom center of the screen."
        );
        eprintln!(
            "Speech model: {}. Tray → Speech model / ASR CPU threads to change settings.",
            init.asr_model.tray_summary()
        );

        while running.load(Ordering::Relaxed) {
            dispatch_pending_messages(&mut msg);

            match status_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(TrayCommand::SetPhase(phase)) => {
                    let next = tray_icon_state(phase);
                    if next != current_icon {
                        current_icon = next;
                        if let Ok(icon) = build_tray_icon(current_icon) {
                            let _ = tray.set_icon(Some(icon));
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    Ok(())
}

fn threads_label(threads: usize) -> String {
    if threads == 0 {
        format!("auto ({})", recommended_thread_count())
    } else {
        threads.to_string()
    }
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
    use crate::config::ModelTier;

    #[test]
    fn moonshine_tier_maps_to_asr_model() {
        for tier in ModelTier::ALL {
            let model = AsrModelId::moonshine(tier);
            assert_eq!(model.moonshine_tier(), Some(tier));
        }
    }
}
