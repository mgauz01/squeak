use squeak::postprocess::{
    detect_context_from_process, postprocess, postprocess_with_polisher, InputContext,
    MockGrammarPolisher, PostProcessOptions,
};

#[test]
fn ae1_prose_strips_um_and_punctuates() {
    let out = postprocess(
        "um send the report by Friday",
        PostProcessOptions {
            context: InputContext::Prose,
            ..Default::default()
        },
    );
    assert_eq!(out, "Send the report by Friday.");
}

#[test]
fn ae9_code_context_keeps_like() {
    let ctx = detect_context_from_process("Cursor.exe");
    let out = postprocess(
        "like um refactor the handler",
        PostProcessOptions {
            context: ctx,
            ..Default::default()
        },
    );
    assert!(out.contains("like"));
    assert!(!out.to_ascii_lowercase().contains("um"));
    assert!(!out.ends_with('.'));
}

#[test]
fn empty_input_stays_empty() {
    let out = postprocess("   ", PostProcessOptions::default());
    assert_eq!(out, "");
}

#[test]
fn ae9_code_context_skips_grammar_even_when_enabled() {
    let ctx = detect_context_from_process("Cursor.exe");
    let mut mock = MockGrammarPolisher::with_suffix(" [grammar]");
    let out = postprocess_with_polisher(
        "like um refactor the handler",
        PostProcessOptions {
            context: ctx,
            grammar_enabled: true,
        },
        Some(&mut mock),
    );
    assert!(!out.contains("[grammar]"));
    assert!(out.contains("like"));
}

#[test]
fn grammar_runs_for_prose_before_punctuation() {
    let mut mock = MockGrammarPolisher::with_suffix(" fixed");
    let out = postprocess_with_polisher(
        "um send the report by Friday",
        PostProcessOptions {
            context: InputContext::Prose,
            grammar_enabled: true,
        },
        Some(&mut mock),
    );
    assert!(out.contains(" fixed"));
    assert!(out.ends_with('.'));
    assert!(!out.to_ascii_lowercase().contains("um"));
}

#[test]
fn grammar_failure_falls_back_to_rules_only() {
    let mut mock = MockGrammarPolisher::failing();
    let out = postprocess_with_polisher(
        "send the report by Friday",
        PostProcessOptions {
            context: InputContext::Prose,
            grammar_enabled: true,
        },
        Some(&mut mock),
    );
    assert_eq!(out, "Send the report by Friday.");
}

#[test]
fn grammar_disabled_never_calls_polisher() {
    let mut mock = MockGrammarPolisher::with_suffix("!");
    let out = postprocess_with_polisher(
        "hello world",
        PostProcessOptions {
            context: InputContext::Prose,
            grammar_enabled: false,
        },
        Some(&mut mock),
    );
    assert_eq!(out, "Hello world.");
}
