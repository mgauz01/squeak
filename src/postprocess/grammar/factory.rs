use std::path::Path;

use crate::config::GrammarModelId;
use crate::postprocess::grammar::engine::{GrammarError, GrammarPolisher};

#[cfg(feature = "gec-coedit")]
use crate::postprocess::grammar::coedit::CoeditPolisher;
#[cfg(feature = "gec-llama")]
use crate::postprocess::grammar::llama::LlamaGrammarPolisher;
#[cfg(feature = "gec-tiny")]
use crate::postprocess::grammar::tiny_t5::TinyT5Polisher;

pub fn create_polisher(
    model: GrammarModelId,
    model_dir: &Path,
) -> Result<Box<dyn GrammarPolisher>, GrammarError> {
    match model {
        GrammarModelId::Tiny => {
            #[cfg(feature = "gec-tiny")]
            {
                Ok(Box::new(TinyT5Polisher::load_from_dir(model_dir)?))
            }
            #[cfg(not(feature = "gec-tiny"))]
            {
                let _ = model_dir;
                Err(GrammarError::Unavailable)
            }
        }
        #[cfg(feature = "gec-coedit")]
        GrammarModelId::Coedit => Ok(Box::new(CoeditPolisher::load_from_dir(model_dir)?)),
        #[cfg(feature = "gec-llama")]
        GrammarModelId::Llama => Ok(Box::new(LlamaGrammarPolisher::load_from_dir(model_dir)?)),
    }
}
