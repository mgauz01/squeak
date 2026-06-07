use super::context::InputContext;

const SENTENCE_ENDINGS: &[char] = &['.', '!', '?'];

pub fn apply_punctuation(text: &str, context: InputContext) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match context {
        InputContext::CodeEditor => trimmed.to_string(),
        InputContext::Prose => ensure_sentence_end(&capitalize_first(trimmed)),
    }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::with_capacity(text.len());
    out.extend(first.to_uppercase());
    out.extend(chars);
    out
}

fn ensure_sentence_end(text: &str) -> String {
    if text.ends_with(SENTENCE_ENDINGS) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postprocess::context::InputContext;

    #[test]
    fn prose_adds_period_and_caps() {
        let out = apply_punctuation("hello world", InputContext::Prose);
        assert_eq!(out, "Hello world.");
    }

    #[test]
    fn code_leaves_text_untouched() {
        let out = apply_punctuation("helloWorld", InputContext::CodeEditor);
        assert_eq!(out, "helloWorld");
    }
}
