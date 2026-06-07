use std::path::PathBuf;

use super::model::{AsrModelId, ModelTier};
use super::grammar::GrammarModelId;

pub fn config_dir() -> PathBuf {
    config_base().join("Squeak")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn models_dir() -> PathBuf {
    data_local_base().join("Squeak").join("models")
}

fn config_base() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("HOME").map(|home| PathBuf::from(home).join(".config"))
            })
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

fn data_local_base() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

pub fn model_dir_for(model: AsrModelId) -> PathBuf {
    models_dir().join(model.directory_name())
}

pub fn grammar_model_dir_for(model: GrammarModelId) -> PathBuf {
    models_dir().join(model.directory_name())
}

/// Legacy helper — Moonshine tier directories at `models/<tier-dir>/`.
pub fn model_dir(tier: ModelTier) -> PathBuf {
    model_dir_for(AsrModelId::moonshine(tier))
}
