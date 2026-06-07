use super::context::InputContext;

const PROSE_FILLERS: &[&str] = &[
    "um",
    "uh",
    "er",
    "ah",
    "like",
    "you know",
    "i mean",
    "sort of",
    "kind of",
];

const CODE_FILLERS: &[&str] = &["um", "uh"];

pub fn strip_fillers(text: &str, context: InputContext) -> String {
    let fillers = match context {
        InputContext::Prose => PROSE_FILLERS,
        InputContext::CodeEditor => CODE_FILLERS,
    };

    let mut result = text.to_string();
    for filler in fillers {
        result = remove_filler_phrase(&result, filler);
    }
    normalize_whitespace(&result)
}

fn remove_filler_phrase(text: &str, filler: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lower: String = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if lower[i..].starts_with(filler)
            && is_boundary_before(&lower, i)
            && is_boundary_after(&lower, i + filler.len())
        {
            i += filler.len();
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_boundary_before(lower: &str, idx: usize) -> bool {
    idx == 0 || !lower.as_bytes()[idx - 1].is_ascii_alphanumeric()
}

fn is_boundary_after(lower: &str, idx: usize) -> bool {
    idx >= lower.len() || !lower.as_bytes()[idx].is_ascii_alphanumeric()
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
}
