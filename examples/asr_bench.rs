//! Compare speech models on the same WAV (Windows):
//!
//! ```powershell
//! cargo run --example asr_bench --release -- C:\path\to\16khz-mono.wav
//! cargo run --example asr_bench --release -- clip.wav --models moonshine:tiny,moonshine:small
//! cargo run --example asr_bench --features parakeet --release -- clip.wav --models parakeet
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!("asr_bench requires Windows (ONNX + model download).");
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
    use std::path::Path;
    use std::time::Instant;

    use squeak::asr::AsrWorker;
    use squeak::config::AsrModelId;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut args = env::args().skip(1);
    let wav_path = args
        .next()
        .ok_or(
            "usage: asr_bench <16khz-mono.wav> [--models moonshine:small,parakeet]\n\
             legacy:   asr_bench clip.wav --tiers tiny,small,medium",
        )?;

    let models = parse_models(args.next().as_deref())?;
    let wav = Path::new(&wav_path);
    if !wav.is_file() {
        return Err(format!("WAV file not found: {wav_path}").into());
    }

    let samples = load_wav_mono_16k(wav)?;
    println!(
        "Loaded {} samples ({:.2}s) from {}",
        samples.len(),
        samples.len() as f64 / 16_000.0,
        wav.display()
    );
    println!("---");

    let config = squeak::config::Config::load();
    let worker = AsrWorker::spawn(config.directml);

    for model in models {
        println!("Model: {}", model.config_key());
        worker.ensure_ready(model)?;
        let start = Instant::now();
        let text = worker.transcribe(samples.clone())?;
        let elapsed = start.elapsed();
        println!("  Time: {} ms", elapsed.as_millis());
        println!("  Text: {text}");
        println!("---");
    }

    Ok(())
}

#[cfg(windows)]
fn parse_models(arg: Option<&str>) -> Result<Vec<AsrModelId>, Box<dyn std::error::Error>> {
    let Some(raw) = arg else {
        return Ok(AsrModelId::MOONSHINE_ALL.to_vec());
    };

    let list = raw
        .strip_prefix("--models")
        .or_else(|| raw.strip_prefix("--tiers"))
        .ok_or_else(|| format!("unexpected argument: {raw}"))?;
    let list = list.trim_start_matches('=').trim();
    if list.is_empty() {
        return Err("missing model list after --models".into());
    }

    let mut models = Vec::new();
    for part in list.split(',') {
        let part = part.trim();
        let model = AsrModelId::parse(part)
            .ok_or_else(|| format!("unknown model: {part} (try moonshine:small or tiny)"))?;
        models.push(model);
    }
    Ok(models)
}

#[cfg(windows)]
fn load_wav_mono_16k(path: &std::path::Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    use hound::{SampleFormat, WavReader};

    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(format!("expected 16 kHz WAV, got {} Hz", spec.sample_rate).into());
    }
    if spec.channels != 1 {
        return Err(format!("expected mono WAV, got {} channels", spec.channels).into());
    }

    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<_, _>>()?,
    };
    Ok(samples)
}
