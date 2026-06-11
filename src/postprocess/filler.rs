use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

use super::context::InputContext;

const PROSE_FILLERS: &[&str] = &[
    "um", "uh", "er", "ah", "like", "you know", "i mean", "sort of", "kind of",
];

const CODE_FILLERS: &[&str] = &["um", "uh"];

static PROSE_REGEX: OnceLock<Regex> = OnceLock::new();
static CODE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_filler_regex(context: InputContext) -> &'static Regex {
    match context {
        InputContext::Prose => PROSE_REGEX.get_or_init(|| build_regex(PROSE_FILLERS)),
        InputContext::CodeEditor => CODE_REGEX.get_or_init(|| build_regex(CODE_FILLERS)),
    }
}

fn build_regex(fillers: &[&str]) -> Regex {
    // Build a regex that matches any of the fillers as whole words, case-insensitively.
    // \b is a word boundary.
    let patterns: Vec<String> = fillers.iter().map(|f| format!(r"\b{}\b\s*", f)).collect();
    let pattern = format!(r"(?i){}", patterns.join("|"));
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .expect("Failed to build filler regex")
}

pub fn strip_fillers(text: &str, context: InputContext) -> String {
    if text.is_empty() {
        return String::new();
    }

    let re = get_filler_regex(context);
    // Replace all fillers with an empty string.
    // Regex::replace_all returns a Cow<str>, avoiding allocation if no matches.
    let stripped = re.replace_all(text, "");

    normalize_whitespace(&stripped)
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postprocess::context::InputContext;

    #[test]
    fn prose_strips_like() {
        let out = strip_fillers("um hello like world", InputContext::Prose);
        assert!(!out.contains("like"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn code_keeps_like() {
        let out = strip_fillers("like um hello", InputContext::CodeEditor);
        assert!(out.contains("like"));
        assert!(!out.contains("um"));
    }

    #[test]
    fn handles_punctuation() {
        let out = strip_fillers("Hello, um, world!", InputContext::Prose);
        assert_eq!(out, "Hello, , world!");
    }
}
