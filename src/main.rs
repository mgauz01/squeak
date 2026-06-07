use squeak::timing;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(windows)]
    {
        if let Err(err) = run_windows() {
            tracing::error!("{err}");
            eprintln!("Squeak exited with error: {err}");
            std::process::exit(1);
        }
    }

    #[cfg(not(windows))]
    {
        eprintln!(
            "Squeak is a Windows tray application. \
             Run `cargo test` for portable modules. \
             (PTT min hold: {} ms, double-tap: {} ms)",
            timing::PTT_MIN_HOLD_MS,
            timing::DOUBLE_TAP_WINDOW_MS,
        );
        std::process::exit(0);
    }
}

#[cfg(windows)]
fn run_windows() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = match squeak::app::AppRuntime::start() {
        Ok(runtime) => runtime,
        Err(err) => {
            if err.to_string().contains("already running") {
                eprintln!("Squeak is already running. Look for the orange tray icon (^ overflow in the taskbar).");
            } else {
                eprintln!("Squeak failed to start: {err}");
            }
            return Err(err);
        }
    };
    runtime.run()
}
