use std::path::Path;

use crate::config::GrammarModelId;
use crate::postprocess::grammar::engine::{GrammarError, GrammarPolisher};
use crate::postprocess::grammar::provision::model_is_complete;
use crate::postprocess::grammar::t5_onnx::T5OnnxGec;

const COEDIT_PREFIX: &str = "Fix grammatical errors in this sentence: ";

pub struct CoeditPolisher {
    inner: T5OnnxGec,
}

impl CoeditPolisher {
    pub fn load_from_dir(dir: &Path) -> Result<Self, GrammarError> {
        if !model_is_complete(GrammarModelId::Coedit, dir) {
            return Err(GrammarError::Other(format!(
                "CoEdIT model files missing in {}",
                dir.display()
            )));
        }
        let inner = T5OnnxGec::load_from_dir(dir, COEDIT_PREFIX)?;
        Ok(Self { inner })
    }
}

impl GrammarPolisher for CoeditPolisher {
    fn is_loaded(&self) -> bool {
        true
    }

    fn model_id(&self) -> Option<GrammarModelId> {
        Some(GrammarModelId::Coedit)
    }

    fn polish(&mut self, text: &str) -> Result<String, GrammarError> {
        self.inner.correct(text)
    }
}
