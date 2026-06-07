mod context;
mod filler;
mod punctuation;

pub use context::{detect_context_from_process, InputContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostProcessOptions {
    pub context: InputContext,
}

impl Default for PostProcessOptions {
    fn default() -> Self {
        Self {
            context: InputContext::Prose,
        }
    }
}

/// Rules-only cleanup: filler removal then punctuation heuristics.
pub fn postprocess(raw: &str, options: PostProcessOptions) -> String {
    let without_fillers = filler::strip_fillers(raw, options.context);
    punctuation::apply_punctuation(&without_fillers, options.context)
}

