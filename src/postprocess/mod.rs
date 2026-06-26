mod context;
mod filler;
mod grammar;
mod punctuation;

pub use context::{detect_context_from_process, InputContext};
pub use filler::strip_fillers;
pub use grammar::engine::MockGrammarPolisher;

#[cfg(windows)]
pub use grammar::GrammarWorker;

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

    let collapsed = collapse_word_repeats(&after_grammar, 2);
    punctuation::apply_punctuation(&collapsed, options.context)
}

/// Caps consecutive duplicate words (case-insensitive). Safety net for ASR replay.
fn collapse_word_repeats(text: &str, max_run: usize) -> String {
    if max_run == 0 {
        return text.to_string();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    let mut out = Vec::with_capacity(words.len());
    let mut run = 0usize;
    let mut prev_lower: Option<String> = None;
    for word in words {
        let lower = word.to_ascii_lowercase();
        if Some(&lower) == prev_lower.as_ref() {
            run += 1;
            if run <= max_run {
                out.push(word);
            }
        } else {
            prev_lower = Some(lower);
            run = 1;
            out.push(word);
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::collapse_word_repeats;

    #[test]
    fn collapse_word_repeats_caps_stutter() {
        assert_eq!(collapse_word_repeats("Ooh la la la la la", 2), "Ooh la la");
    }
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

    let collapsed = collapse_word_repeats(&after_grammar, 2);
    punctuation::apply_punctuation(&collapsed, options.context)
}
