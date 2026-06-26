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
    use std::time::Instant;

    use squeak::config::GrammarModelId;
    use squeak::postprocess::GrammarWorker;

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

        let mut times_ms = Vec::with_capacity(samples.len());
        for sample in &samples {
            let start = Instant::now();
            let out = worker.polish(sample)?;
            let elapsed_ms = start.elapsed().as_millis();
            times_ms.push(elapsed_ms);
            println!("IN:  {sample}");
            println!("OUT: {out}");
            println!("  Time: {elapsed_ms} ms\n");
        }

        print_summary(&times_ms);
        worker.reload(model)?;
    }

    Ok(())
}

#[cfg(windows)]
fn print_summary(times_ms: &[u128]) {
    if times_ms.is_empty() {
        return;
    }
    let total: u128 = times_ms.iter().sum();
    let min = *times_ms.iter().min().unwrap();
    let max = *times_ms.iter().max().unwrap();
    let mut sorted = times_ms.to_vec();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let avg = total / times_ms.len() as u128;
    println!(
        "Summary ({} samples): total {total} ms, avg {avg} ms, median {median} ms, min {min} ms, max {max} ms",
        times_ms.len()
    );
}
