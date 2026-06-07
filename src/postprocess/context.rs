#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Prose,
    CodeEditor,
}

/// Detect input context from foreground process executable name (Windows impl later).
pub fn detect_context_from_process(process_name: &str) -> InputContext {
    let lower = process_name.to_ascii_lowercase();
    const CODE_EDITORS: &[&str] = &[
        "code.exe",
        "cursor.exe",
        "devenv.exe",
        "idea64.exe",
        "pycharm64.exe",
        "webstorm64.exe",
        "sublime_text.exe",
        "notepad++.exe",
        "vim.exe",
        "nvim.exe",
    ];
    if CODE_EDITORS.iter().any(|name| lower.ends_with(name)) {
        InputContext::CodeEditor
    } else {
        InputContext::Prose
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cursor_as_code() {
        assert_eq!(
            detect_context_from_process("Cursor.exe"),
            InputContext::CodeEditor
        );
    }

    #[test]
    fn detects_notepad_as_prose() {
        assert_eq!(
            detect_context_from_process("notepad.exe"),
            InputContext::Prose
        );
    }
}
