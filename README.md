<div align="center">

# Squeak

Local voice dictation for Windows. Speech stays on your PC.

<br>

<img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-62105E?style=for-the-badge&logo=windows&logoColor=white">
<img alt="Offline" src="https://img.shields.io/badge/Speech-100%25%20local-962872?style=for-the-badge">
<img alt="License" src="https://img.shields.io/badge/License-MIT-A862B2?style=for-the-badge">

<br><br>

<a href="https://github.com/mgauz01/squeak/releases/latest/download/Squeak-0.1.0-x64.msi">
  <img alt="Download Squeak for Windows (MSI x64)" src="https://img.shields.io/badge/Download%20for%20Windows-MSI%20(x64)-962872?style=for-the-badge&logo=windows&logoColor=white">
</a>

<br><br>

<sub>Purple pill at the bottom of the screen = ready. Hold <strong>Win+Ctrl</strong> to dictate.</sub>

<sub><a href="https://github.com/mgauz01/squeak/releases">Other releases</a></sub>

</div>

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
=======
Squeak types what you say at the cursor: email, docs, chat, editors. Push-to-talk, optional hands-free, light cleanup (fillers and punctuation). Not for meeting transcription or macOS/Linux.

Install the MSI, allow the microphone, launch from the Start Menu. Hold Win+Ctrl while you talk (wait ~300 ms before speaking). Double-tap Win+Ctrl for hands-free; double-tap again to stop. Shift+Alt+Z pastes the last transcript again. Tray menu: model, settings, config.toml, quit.

Models download on first dictation to `%LOCALAPPDATA%\Squeak\models\`. Settings: `%APPDATA%\Squeak\config.toml`.

Recognition off? Hold Win+Ctrl the whole time, try Moonshine Small or Medium in the tray, check Settings → Privacy → Microphone.

---

## Install

Windows 10+, a mic, admin rights for the MSI.

Use the download button above, or grab `Squeak-*-x64.msi` from [Releases](https://github.com/mgauz01/squeak/releases). Installs to `C:\Program Files\Squeak\`. Remove via Settings → Apps.

```powershell
msiexec /i ".\Squeak-0.1.0-x64.msi"
```

Build from source ([Rust](https://rustup.rs/) on Windows): clone, `cargo run --release`. MSI: `.\installer\build.ps1` ([WiX 3.14](https://github.com/wixtoolset/wix3/releases/tag/wix3141rtm), e.g. `choco install wixtoolset --version=3.14.1 -y`).

---

## CI

- **[`ci.yml`](.github/workflows/ci.yml)** — runs on every push to `main` and every pull request targeting `main`:
  - **validate-linux** — `cargo fmt`, Clippy, unit tests
  - **validate-windows** — release compile check, ASR smoke/bench on a committed fixture
  - **build-msi** — builds the MSI only after both validation jobs pass
- **[`msi.yml`](.github/workflows/msi.yml)** — runs on version tags (`v*`) or manual dispatch: same validation + MSI build, then uploads the MSI to the GitHub release.

`main` is branch-protected: merges require **validate-linux**, **validate-windows**, and **build-msi** to pass.

See the [Actions](https://github.com/mgauz01/squeak/actions) tab for workflow runs.

---

## Contributing

Open an [issue](https://github.com/mgauz01/squeak/issues) or [pull request](https://github.com/mgauz01/squeak/pulls). Note what you tested on Windows. Background in [`docs/`](docs/).

---

<div align="center">

<sub>MIT</sub>

</div>
