use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Tiny,
    Small,
}

impl Default for ModelTier {
    fn default() -> Self {
        Self::Tiny
    }
}

impl ModelTier {
    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Tiny => "moonshine-tiny-streaming-en",
            Self::Small => "moonshine-small-streaming-en",
        }
    }

    pub fn download_url(self) -> &'static str {
        match self {
            Self::Tiny => "https://blob.handy.computer/moonshine-tiny-streaming-en.tar.gz",
            Self::Small => "https://blob.handy.computer/moonshine-small-streaming-en.tar.gz",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub model_tier: ModelTier,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default)]
    pub directml: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_tier: ModelTier::default(),
            autostart: true,
            directml: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = super::paths::config_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = super::paths::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = Config::default();
        assert_eq!(cfg.model_tier, ModelTier::Tiny);
        assert!(cfg.autostart);
        assert!(!cfg.directml);
    }
}
