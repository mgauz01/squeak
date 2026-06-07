//! Compare tiny vs CoEdIT grammar backends on fixed sentences (Windows manual):
//!
//! ```powershell
//! cargo run --example gec_bench --release --features gec-tiny,gec-coedit
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!("gec_bench requires Windows.");
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
    use squeak::config::GrammarModelId;
    use squeak::postprocess::grammar::GrammarWorker;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let samples = include_str!("../tests/fixtures/gec_samples.txt")
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();

    let worker = GrammarWorker::spawn();

    for model in [GrammarModelId::Tiny, GrammarModelId::Coedit] {
        println!("\n=== {} ===", model.config_key());
        worker.ensure_ready(model)?;
        for sample in &samples {
            let out = worker.polish(sample)?;
            println!("IN:  {sample}");
            println!("OUT: {out}\n");
        }
        worker.reload(model)?;
    }

    Ok(())
}
