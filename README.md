<div align="center">

# Squeak

**Local voice dictation for Windows — private, fast, no cloud.**

<br>

<img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-62105E?style=for-the-badge&logo=windows&logoColor=white">
<img alt="Offline" src="https://img.shields.io/badge/Speech-100%25%20local-962872?style=for-the-badge">
<img alt="License" src="https://img.shields.io/badge/License-MIT-A862B2?style=for-the-badge">

<br><br>

<sub>A purple pill at the bottom of your screen means Squeak is listening for <strong>Win+Ctrl</strong>.</sub>

</div>

---

## What is Squeak?

Squeak turns your speech into text **where your cursor already is** — email, docs, chat, code editors. Everything runs on your PC: no audio sent to the cloud, no subscription.

It is a focused Wispr-style dictation tool: push-to-talk, optional hands-free mode, light cleanup (fillers and punctuation), and a small animated overlay so you always know when the mic is live.

**Good for:** quick dictation, accessibility, reducing typing strain, drafting prose anywhere you can type.

**Not for:** meeting transcription, multi-user servers, or macOS/Linux (Windows only today).

---

## How to use

1. **Install and launch** Squeak (see below). Allow microphone access when Windows asks.
2. Look for the **purple pill** along the bottom edge of your screen and the **tray icon** in the taskbar.
3. **Dictate:**
   - **Push-to-talk** — hold **Win+Ctrl**, speak, release. Hold at least ~300 ms so recording starts.
   - **Hands-free** — double-tap **Win+Ctrl** to start; double-tap again to stop.
4. Text is typed at the caret. **Shift+Alt+Z** pastes the last transcript again.
5. **Tray menu** — speech model, settings, **Open config.toml**, **Check for updates…**, quit.

On first dictation, speech models download once to `%LOCALAPPDATA%\Squeak\models\` (not included in the installer). Settings live in `%APPDATA%\Squeak\config.toml`.

**Updates (MSI installs):** Tray → **Check for updates…** looks for a newer release on GitHub, asks for confirmation, then downloads the MSI and restarts Squeak.

<details>
<summary><strong>Tips if accuracy feels off</strong></summary>

- Hold **Win+Ctrl** the whole time you are speaking.
- Pick **Moonshine Small** or **Medium** in the tray if you need better quality.
- Grant microphone permission in **Settings → Privacy → Microphone**.

</details>

---

## Install

**Requirements:** Windows 10 or later, a microphone, admin rights for install.

### Option A — Download the MSI (recommended)

1. Get the latest **`Squeak-*-x64.msi`** from [**GitHub Releases**](https://github.com/mgauz01/squeak/releases).
2. Run the installer (UAC prompt). Squeak installs to `C:\Program Files\Squeak\` and adds a Start Menu shortcut.
3. Launch **Squeak** from the Start Menu.

```powershell
# Or from a shell (elevated):
msiexec /i ".\Squeak-0.1.0-x64.msi"
```

Uninstall via **Settings → Apps → Squeak**.

### Option B — Build from source

For developers who already have [Rust](https://rustup.rs/) on Windows:

```powershell
git clone https://github.com/mgauz01/squeak.git
cd squeak
cargo run --release
```

To package an MSI yourself: `.\installer\build.ps1` (needs [WiX Toolset 3.14](https://github.com/wixtoolset/wix3/releases/tag/wix3141rtm), e.g. `choco install wixtoolset --version=3.14.1 -y`).

---

## CI

- **[`ci.yml`](.github/workflows/ci.yml)** — runs on every push to `main` and every pull request targeting `main`: `cargo fmt`, Clippy, and unit tests on Linux.
- **[`msi.yml`](.github/workflows/msi.yml)** — runs on version tags (`v*`) or manual dispatch: Linux + Windows validation (including ASR smoke/bench on a committed fixture), then builds the MSI only if validation passes.

See the [Actions](https://github.com/mgauz01/squeak/actions) tab for workflow runs.

---

## Contributing

Squeak is open source and **contributions are welcome**.

- **Bug reports & ideas** — [open an issue](https://github.com/mgauz01/squeak/issues)
- **Code** — fork, branch, and [open a pull request](https://github.com/mgauz01/squeak/pulls)

Please keep changes focused and include a short description of what you tested on Windows. Design docs and plans live under [`docs/`](docs/) if you want deeper context before diving in.

---

<div align="center">

<sub>MIT · <a href="docs/">Developer docs</a></sub>

</div>
