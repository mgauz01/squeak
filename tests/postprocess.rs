use squeak::postprocess::{detect_context_from_process, postprocess, InputContext, PostProcessOptions};

#[test]
fn ae1_prose_strips_um_and_punctuates() {
    let out = postprocess(
        "um send the report by Friday",
        PostProcessOptions {
            context: InputContext::Prose,
        },
    );
    assert_eq!(out, "Send the report by Friday.");
}

#[test]
fn ae9_code_context_keeps_like() {
    let ctx = detect_context_from_process("Cursor.exe");
    let out = postprocess(
        "like um refactor the handler",
        PostProcessOptions { context: ctx },
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
