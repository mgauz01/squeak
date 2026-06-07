use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

/// Identifies a local ASR backend and variant (size tier or default profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrModelId {
    Moonshine(ModelTier),
    #[cfg(feature = "parakeet")]
    Parakeet,
    #[cfg(feature = "cohere")]
    Cohere,
    #[cfg(feature = "canary")]
    Canary,
}

impl Default for AsrModelId {
    fn default() -> Self {
        Self::Moonshine(ModelTier::default())
    }
}

impl AsrModelId {
    pub fn moonshine(tier: ModelTier) -> Self {
        Self::Moonshine(tier)
    }

    pub fn moonshine_tier(self) -> Option<ModelTier> {
        match self {
            Self::Moonshine(tier) => Some(tier),
            #[cfg(any(feature = "parakeet", feature = "cohere", feature = "canary"))]
            _ => None,
        }
    }

    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Moonshine(tier) => tier.directory_name(),
            #[cfg(feature = "parakeet")]
            Self::Parakeet => "parakeet-tdt-0.6b-v3-int8",
            #[cfg(feature = "cohere")]
            Self::Cohere => "cohere-transcribe-int8",
            #[cfg(feature = "canary")]
            Self::Canary => "canary-1b-v2-int8",
        }
    }

    pub fn download_url(self) -> Option<&'static str> {
        match self {
            Self::Moonshine(tier) => Some(tier.download_url()),
            #[cfg(feature = "parakeet")]
            Self::Parakeet => Some("https://blob.handy.computer/parakeet-v3-int8.tar.gz"),
            #[cfg(feature = "cohere")]
            Self::Cohere => Some("https://blob.handy.computer/cohere-transcribe-int8.tar.gz"),
            #[cfg(feature = "canary")]
            Self::Canary => None,
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Moonshine(tier) => tier.menu_label(),
            #[cfg(feature = "parakeet")]
            Self::Parakeet => "Parakeet (efficient, ~6% WER)",
            #[cfg(feature = "cohere")]
            Self::Cohere => "Cohere (maximum accuracy, ~5% WER)",
            #[cfg(feature = "canary")]
            Self::Canary => "Canary (accuracy mode)",
        }
    }

    pub fn tray_summary(self) -> String {
        match self {
            Self::Moonshine(tier) => format!("Moonshine {tier:?}"),
            #[cfg(feature = "parakeet")]
            Self::Parakeet => "Parakeet".into(),
            #[cfg(feature = "cohere")]
            Self::Cohere => "Cohere".into(),
            #[cfg(feature = "canary")]
            Self::Canary => "Canary".into(),
        }
    }

    /// Parse tray/config strings: `small`, `moonshine:medium`, `parakeet`, etc.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        if let Some(tier) = ModelTier::parse(&s) {
            return Some(Self::moonshine(tier));
        }
        if let Some((backend, variant)) = s.split_once(':') {
            return match backend {
                "moonshine" => ModelTier::parse(variant).map(Self::moonshine),
                #[cfg(feature = "parakeet")]
                "parakeet" if variant.is_empty() || variant == "default" => Some(Self::Parakeet),
                #[cfg(feature = "cohere")]
                "cohere" if variant.is_empty() || variant == "default" => Some(Self::Cohere),
                #[cfg(feature = "canary")]
                "canary" if variant.is_empty() || variant == "default" => Some(Self::Canary),
                _ => None,
            };
        }
        match s.as_str() {
            #[cfg(feature = "parakeet")]
            "parakeet" => Some(Self::Parakeet),
            #[cfg(feature = "cohere")]
            "cohere" => Some(Self::Cohere),
            #[cfg(feature = "canary")]
            "canary" => Some(Self::Canary),
            _ => None,
        }
    }

    pub fn config_key(self) -> String {
        match self {
            Self::Moonshine(tier) => format!("moonshine:{tier:?}").to_lowercase(),
            #[cfg(feature = "parakeet")]
            Self::Parakeet => "parakeet".into(),
            #[cfg(feature = "cohere")]
            Self::Cohere => "cohere".into(),
            #[cfg(feature = "canary")]
            Self::Canary => "canary".into(),
        }
    }

    pub const MOONSHINE_ALL: [Self; 3] = [
        Self::Moonshine(ModelTier::Tiny),
        Self::Moonshine(ModelTier::Small),
        Self::Moonshine(ModelTier::Medium),
    ];
}

impl Serialize for AsrModelId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.config_key())
    }
}

impl<'de> Deserialize<'de> for AsrModelId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown asr model: {s}")))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    asr_model: AsrModelId,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default)]
    pub directml: bool,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            asr_model: Option<AsrModelId>,
            #[serde(default)]
            model_tier: Option<ModelTier>,
            #[serde(default = "default_true")]
            autostart: bool,
            #[serde(default)]
            directml: bool,
        }

        let raw = Raw::deserialize(deserializer)?;
        let asr_model = raw.asr_model.unwrap_or_else(|| {
            raw.model_tier
                .map(AsrModelId::moonshine)
                .unwrap_or_default()
        });

        Ok(Self {
            asr_model,
            autostart: raw.autostart,
            directml: raw.directml,
        })
    }
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            asr_model: AsrModelId::default(),
            autostart: true,
            directml: false,
        }
    }
}

impl Config {
    pub fn asr_model(&self) -> AsrModelId {
        self.asr_model
    }

    pub fn set_asr_model(&mut self, model: AsrModelId) {
        self.asr_model = model;
    }

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
        assert_eq!(cfg.asr_model(), AsrModelId::moonshine(ModelTier::Small));
        assert!(cfg.autostart);
        assert!(!cfg.directml);
    }

    #[test]
    fn legacy_model_tier_deserializes_to_moonshine() {
        let raw = r#"
model_tier = "medium"
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.asr_model(), AsrModelId::moonshine(ModelTier::Medium));
    }

    #[test]
    fn asr_model_round_trip() {
        let cfg = Config {
            asr_model: AsrModelId::moonshine(ModelTier::Tiny),
            ..Config::default()
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        assert!(toml_str.contains("asr_model"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.asr_model(), AsrModelId::moonshine(ModelTier::Tiny));
    }

    #[test]
    fn asr_model_id_parse_moonshine_tiers() {
        assert_eq!(
            AsrModelId::parse("small"),
            Some(AsrModelId::moonshine(ModelTier::Small))
        );
        assert_eq!(
            AsrModelId::parse("moonshine:medium"),
            Some(AsrModelId::moonshine(ModelTier::Medium))
        );
        assert_eq!(AsrModelId::parse("unknown"), None);
    }

    #[test]
    fn asr_model_id_moonshine_tier() {
        let id = AsrModelId::moonshine(ModelTier::Small);
        assert_eq!(id.moonshine_tier(), Some(ModelTier::Small));
    }

    #[test]
    fn model_tier_parse() {
        assert_eq!(ModelTier::parse("small"), Some(ModelTier::Small));
        assert_eq!(ModelTier::parse("Medium"), Some(ModelTier::Medium));
        assert_eq!(ModelTier::parse("unknown"), None);
    }
}
