//! Manual grammar-correction smoke test on Windows:
//!
//! ```powershell
//! cargo run --example gec_smoke --release --features gec-tiny -- "i goes to the store yesterday"
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!("gec_smoke requires Windows (ONNX + model download).");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::env;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let text = env::args()
        .nth(1)
        .unwrap_or_else(|| "i goes to the store yesterday".to_string());

    let worker = squeak::postprocess::grammar::GrammarWorker::spawn();
    let model = squeak::config::GrammarModelId::Tiny;

    println!("Ensuring grammar model ({})...", model.config_key());
    worker.ensure_ready(model)?;

    println!("Input:  {text}");
    let polished = worker.polish(&text)?;
    println!("Output: {polished}");
    Ok(())
}
