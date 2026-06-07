use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Local grammar-correction backend profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarModelId {
    Tiny,
    #[cfg(feature = "gec-coedit")]
    Coedit,
    #[cfg(feature = "gec-llama")]
    Llama,
}

impl Default for GrammarModelId {
    fn default() -> Self {
        Self::Tiny
    }
}

impl GrammarModelId {
    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Tiny => "t5-tiny-gec-hone-int8",
            #[cfg(feature = "gec-coedit")]
            Self::Coedit => "coedit-small-int8",
            #[cfg(feature = "gec-llama")]
            Self::Llama => "grammar-llama-3.2-1b-q4",
        }
    }

    pub fn download_url(self) -> Option<&'static str> {
        match self {
            Self::Tiny => None,
            #[cfg(feature = "gec-coedit")]
            Self::Coedit => None,
            #[cfg(feature = "gec-llama")]
            Self::Llama => None,
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny (fast, ~100 ms)",
            #[cfg(feature = "gec-coedit")]
            Self::Coedit => "CoEdIT (quality, ~200 ms)",
            #[cfg(feature = "gec-llama")]
            Self::Llama => "Llama grammar (~1 GB RAM)",
        }
    }

    pub fn tray_summary(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny",
            #[cfg(feature = "gec-coedit")]
            Self::Coedit => "CoEdIT",
            #[cfg(feature = "gec-llama")]
            Self::Llama => "Llama",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "tiny" | "gec-tiny" => Some(Self::Tiny),
            #[cfg(feature = "gec-coedit")]
            "coedit" | "gec-coedit" => Some(Self::Coedit),
            #[cfg(feature = "gec-llama")]
            "llama" | "gec-llama" => Some(Self::Llama),
            _ => None,
        }
    }

    pub fn config_key(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            #[cfg(feature = "gec-coedit")]
            Self::Coedit => "coedit",
            #[cfg(feature = "gec-llama")]
            Self::Llama => "llama",
        }
    }

    pub fn all_models() -> &'static [Self] {
        &[
            Self::Tiny,
            #[cfg(feature = "gec-coedit")]
            Self::Coedit,
            #[cfg(feature = "gec-llama")]
            Self::Llama,
        ]
    }
}

impl Serialize for GrammarModelId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.config_key())
    }
}

impl<'de> Deserialize<'de> for GrammarModelId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown grammar model: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_model_id_parse_tiny() {
        assert_eq!(GrammarModelId::parse("tiny"), Some(GrammarModelId::Tiny));
        assert_eq!(GrammarModelId::parse("unknown"), None);
    }

    #[test]
    fn grammar_model_id_config_key_round_trip() {
        let id = GrammarModelId::Tiny;
        assert_eq!(GrammarModelId::parse(id.config_key()), Some(id));
    }
}
