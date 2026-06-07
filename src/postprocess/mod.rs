mod context;
mod filler;
mod grammar;
mod punctuation;

pub use context::{detect_context_from_process, InputContext};
pub use grammar::engine::MockGrammarPolisher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostProcessOptions {
    pub context: InputContext,
    pub grammar_enabled: bool,
}

impl Default for PostProcessOptions {
    fn default() -> Self {
        Self {
            context: InputContext::Prose,
            grammar_enabled: false,
        }
    }
}

/// Rules-only cleanup: filler removal then punctuation heuristics.
pub fn postprocess(raw: &str, options: PostProcessOptions) -> String {
    postprocess_with_polisher(raw, options, None)
}

/// Optional grammar pass (prose only) between fillers and punctuation.
pub fn postprocess_with_polisher(
    raw: &str,
    options: PostProcessOptions,
    polisher: Option<&mut dyn grammar::engine::GrammarPolisher>,
) -> String {
    let without_fillers = filler::strip_fillers(raw, options.context);

    let after_grammar = if options.grammar_enabled && options.context == InputContext::Prose {
        if without_fillers.trim().is_empty() {
            without_fillers
        } else if let Some(p) = polisher {
            match p.polish(without_fillers.trim()) {
                Ok(text) => text,
                Err(_) => without_fillers,
            }
        } else {
            without_fillers
        }
    } else {
        without_fillers
    };

    punctuation::apply_punctuation(&after_grammar, options.context)
}

#[cfg(windows)]
pub fn postprocess_with_worker(
    raw: &str,
    options: PostProcessOptions,
    worker: Option<&grammar::GrammarWorker>,
    grammar_model: crate::config::GrammarModelId,
) -> String {
    use grammar::worker::polish_or_fallback;

    let without_fillers = filler::strip_fillers(raw, options.context);

    let after_grammar = if options.grammar_enabled && options.context == InputContext::Prose {
        if without_fillers.trim().is_empty() {
            without_fillers
        } else if let Some(w) = worker {
            polish_or_fallback(w, without_fillers.trim(), grammar_model)
        } else {
            without_fillers
        }
    } else {
        without_fillers
    };

    punctuation::apply_punctuation(&after_grammar, options.context)
}
