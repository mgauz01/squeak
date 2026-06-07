use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Tiny,
    Small,
    Medium,
}

impl Default for ModelTier {
    fn default() -> Self {
        Self::Small
    }
}

impl ModelTier {
    pub const ALL: [Self; 3] = [Self::Tiny, Self::Small, Self::Medium];

    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Tiny => "moonshine-tiny-streaming-en",
            Self::Small => "moonshine-small-streaming-en",
            Self::Medium => "moonshine-medium-streaming-en",
        }
    }

    pub fn download_url(self) -> &'static str {
        match self {
            Self::Tiny => "https://blob.handy.computer/moonshine-tiny-streaming-en.tar.gz",
            Self::Small => "https://blob.handy.computer/moonshine-small-streaming-en.tar.gz",
            Self::Medium => "https://blob.handy.computer/moonshine-medium-streaming-en.tar.gz",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny (fastest, ~12% WER)",
            Self::Small => "Small (recommended, ~8% WER)",
            Self::Medium => "Medium (best accuracy, ~7% WER)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "tiny" => Some(Self::Tiny),
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            _ => None,
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
        assert_eq!(cfg.model_tier, ModelTier::Small);
        assert!(cfg.autostart);
        assert!(!cfg.directml);
    }

    #[test]
    fn model_tier_serde_in_config() {
        for tier in ModelTier::ALL {
            let cfg = Config {
                model_tier: tier,
                ..Config::default()
            };
            let toml_str = toml::to_string(&cfg).unwrap();
            let parsed: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(parsed.model_tier, tier);
        }
    }

    #[test]
    fn model_tier_parse() {
        assert_eq!(ModelTier::parse("small"), Some(ModelTier::Small));
        assert_eq!(ModelTier::parse("Medium"), Some(ModelTier::Medium));
        assert_eq!(ModelTier::parse("unknown"), None);
    }
}
