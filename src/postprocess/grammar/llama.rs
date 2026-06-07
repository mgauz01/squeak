use std::path::Path;

use crate::config::GrammarModelId;
use crate::postprocess::grammar::engine::{GrammarError, GrammarPolisher};
use crate::postprocess::grammar::provision::model_is_complete;

pub struct LlamaGrammarPolisher;

impl LlamaGrammarPolisher {
    pub fn load_from_dir(dir: &Path) -> Result<Self, GrammarError> {
        if !model_is_complete(GrammarModelId::Llama, dir) {
            return Err(GrammarError::Other(format!(
                "grammar-Llama GGUF missing in {}",
                dir.display()
            )));
        }
        Ok(Self)
    }
}

impl GrammarPolisher for LlamaGrammarPolisher {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<GrammarModelId> {
        Some(GrammarModelId::Llama)
    }

    fn polish(&mut self, _text: &str) -> Result<String, GrammarError> {
        Err(GrammarError::Polish(
            "gec-llama GGUF inference is not wired yet — model downloads successfully; use gec-tiny or gec-coedit".into(),
        ))
    }
}
