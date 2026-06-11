use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tracing::info;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::app::{AppEvent, UserAction};
use crate::asr::recommended_thread_count;
use crate::config::{AsrModelId, Config, GrammarModelId};
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

impl TrayInit {
    pub fn from_config(config: &Config) -> Self {
        Self {
            asr_model: config.asr_model(),
            grammar_enabled: config.grammar_enabled(),
            grammar_model: config.grammar_model(),
            directml: config.directml,
            xnnpack: config.xnnpack,
            asr_threads: config.asr_threads,
            autostart: config.autostart,
        }
    }
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

fn append_checked_choice<T: Copy + PartialEq>(
    menu: &Submenu,
    label: &str,
    value: T,
    active: T,
    ids: &mut Vec<(muda::MenuId, T)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let item = CheckMenuItem::new(label, true, value == active, None);
    ids.push((item.id().clone(), value));
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

fn on_bool_toggle(
    label: &'static str,
    flag: &AtomicBool,
    tx: &Sender<AppEvent>,
    action: fn(bool) -> UserAction,
) {
    let enabled = !flag.load(Ordering::Relaxed);
    flag.store(enabled, Ordering::Relaxed);
    // CheckMenuItem is !Send/!Sync (Rc<RefCell>), so the menu handler must not
    // capture items — Windows updates the checkmark on click before this runs.
    let _ = tx.send(AppEvent::UserAction(action(enabled)));
    eprintln!(
        "{} {label}…",
        if enabled { "Enabling" } else { "Disabling" }
    );
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
        append_checked_choice(
            &moonshine_menu,
            model.menu_label(),
            model,
            init.asr_model,
            &mut model_ids,
        )?;
    }
    model_menu.append(&moonshine_menu)?;

    #[cfg(feature = "parakeet")]
    append_checked_choice(
        &model_menu,
        AsrModelId::Parakeet.menu_label(),
        AsrModelId::Parakeet,
        init.asr_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "cohere")]
    append_checked_choice(
        &model_menu,
        AsrModelId::Cohere.menu_label(),
        AsrModelId::Cohere,
        init.asr_model,
        &mut model_ids,
    )?;
    #[cfg(feature = "canary")]
    append_checked_choice(
        &model_menu,
        AsrModelId::Canary.menu_label(),
        AsrModelId::Canary,
        init.asr_model,
        &mut model_ids,
    )?;

    let threads_menu = Submenu::new("ASR CPU threads", true);
    let mut thread_ids = Vec::new();
    let auto_threads = recommended_thread_count();
    append_checked_choice(
        &threads_menu,
        &format!("Auto ({auto_threads} threads)"),
        0,
        init.asr_threads,
        &mut thread_ids,
    )?;
    for n in [4_usize, 8, 16] {
        append_checked_choice(
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

    let check_updates_item = MenuItem::new("Check for updates…", true, None);
    let check_updates_id = check_updates_item.id().clone();

    let directml_on = AtomicBool::new(init.directml);
    let xnnpack_on = AtomicBool::new(init.xnnpack);
    let autostart_on = AtomicBool::new(init.autostart);

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
    menu.append(&check_updates_item)?;
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
                    let _ = event_tx_menu
                        .send(AppEvent::UserAction(UserAction::SetAsrThreads(*threads)));
                    eprintln!(
                        "Setting ASR CPU threads to {}…",
                        Config::format_asr_threads(*threads)
                    );
                    return;
                }
            }
            if directml_id == event.id() {
                on_bool_toggle(
                    "DirectML",
                    &directml_on,
                    &event_tx_menu,
                    UserAction::ToggleDirectMl,
                );
                return;
            }
            if xnnpack_id == event.id() {
                on_bool_toggle(
                    "XNNPACK",
                    &xnnpack_on,
                    &event_tx_menu,
                    UserAction::ToggleXnnpack,
                );
                return;
            }
            if autostart_id == event.id() {
                on_bool_toggle(
                    "start with Windows",
                    &autostart_on,
                    &event_tx_menu,
                    UserAction::ToggleAutostart,
                );
                return;
            }
            if open_config_id == event.id() {
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::OpenSettings));
                return;
            }
            if check_updates_id == event.id() {
                let _ = event_tx_menu.send(AppEvent::UserAction(UserAction::CheckForUpdates));
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
