use std::path::PathBuf;

use super::model::{AsrModelId, ModelTier};
use super::grammar::GrammarModelId;

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Squeak")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn models_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Squeak")
        .join("models")
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
