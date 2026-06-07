use std::path::PathBuf;

use super::model::ModelTier;

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

pub fn model_dir(tier: ModelTier) -> PathBuf {
    models_dir().join(tier.directory_name())
}
