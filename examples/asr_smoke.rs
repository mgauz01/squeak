//! Manual ASR smoke test on Windows:
//!
//! ```powershell
//! cargo run --example asr_smoke --release -- C:\path\to\your-recording.wav
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!("asr_smoke requires Windows (ONNX + model download).");
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

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let wav_path = env::args()
        .nth(1)
        .ok_or(
            "usage: asr_smoke <path-to-16khz-mono.wav>\n\
             example: cargo run --example asr_smoke --release -- C:\\Users\\you\\recording.wav",
        )?;

    let wav = Path::new(&wav_path);
    if !wav.is_file() {
        return Err(format!(
            "WAV file not found: {wav_path}\n\
             Replace the README placeholder with a real 16 kHz mono .wav on your machine."
        )
        .into());
    }

    let config = squeak::config::Config::load();
    let worker = squeak::asr::AsrWorker::spawn(config.directml);

    println!("Ensuring model ({:?})...", config.model_tier);
    worker.ensure_ready(config.model_tier)?;

    println!("Transcribing {}...", wav.display());
    let samples = load_wav_mono_16k(wav)?;
    let text = worker.transcribe(samples)?;
    println!("Transcript: {text}");
    Ok(())
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
