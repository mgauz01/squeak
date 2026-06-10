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

## Contributing

Open an [issue](https://github.com/mgauz01/squeak/issues) or [pull request](https://github.com/mgauz01/squeak/pulls). Note what you tested on Windows. Background in [`docs/`](docs/).

---

<div align="center">

<sub>MIT</sub>

</div>
