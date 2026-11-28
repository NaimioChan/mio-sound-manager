# Mio Sound Manager

A local audio sample manager for Windows, built for music production and sound design.

**[简体中文](README.zh-CN.md) · [日本語](README.ja.md)**

![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)
![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-green)
![Version](https://img.shields.io/badge/version-0.3.1-orange)

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri%202-24C8D8?logo=tauri&logoColor=white)
![SolidJS](https://img.shields.io/badge/SolidJS-2C4F7C?logo=solid&logoColor=white)
![Siemens iX](https://img.shields.io/badge/Siemens%20iX-009999)
![SQLite](https://img.shields.io/badge/SQLite%20+%20FTS5-003B57?logo=sqlite&logoColor=white)
![symphonia](https://img.shields.io/badge/symphonia-gray)
![WASAPI / ASIO](https://img.shields.io/badge/WASAPI%20%2F%20ASIO-gray)

---

## Highlights

**Lightweight**  
Installer under 5 MB, single executable, no background services or runtimes required. Low memory footprint, plays nicely alongside your DAW.

**Fast**  
Indexing hundreds of thousands of samples takes around a minute; incremental updates finish in seconds. Search, scroll, and filter feel instant.

**Complete**  
Supports WAV, FLAC, MP3, AIFF, M4A, OGG and more. Reads ID3, BWF, iXML and other embedded tags. Auto-classifies using UCS 8.2.1 and common producer categories. Select a region on the waveform and drag it straight into your DAW.

---

## Features

### Playback
- **WASAPI & ASIO support** — WASAPI shared mode by default (works alongside any DAW without stealing the device); switch to an ASIO driver in settings for lower latency.
- **Auto Play** — select a row to preview immediately (`Alt+A` to toggle).
- **Clean transients** — encoder delay and priming frames from MP3, M4A/AAC are trimmed automatically so drum hits start on time.
- **Playback controls** — volume knob with mute memory, `Space` to pause/resume, `L` to toggle loop.

### Waveform & Drag-out
- **Per-channel waveform** — mono and stereo displayed in separate lanes; click anywhere to seek.
- **Region selection** — drag to select a region, drag the edges to trim; loop plays only the selection when one exists.
- **Drag into DAW** — drag a row to drop the original file; drag from the waveform handle to drop the selected region as a 32-bit float WAV (or the full file if no selection).

### Search, Classification & Libraries
- **Multiple libraries** — add local or external drive folders, session is remembered across restarts; missing drives are kept in the list and re-open when reconnected.
- **Full-text search & filters** — search filenames, paths, descriptions and embedded tags; filter by favorites, category, folder, channels, sample rate, BPM and cover art.
- **Auto-classification** — UCS 8.2.1 (CatID recognition, 82 top-level categories, 753 subcategories) plus 22 common producer categories; BPM parsed from filenames. Manual overrides survive re-indexing. Supports bulk editing and 50-step undo/redo.

### Inspector, Tags & Artwork
- **Inspector panel** — view duration, channels, sample rate, BPM, size, path and category at a glance; collapsible, resizable.
- **Raw tag viewer** — ID3, Vorbis Comments, M4A Atoms, BWF `bext`, iXML, LIST INFO, grouped by source.
- **Cover art** — reads embedded artwork and sidecar images; thumbnail column in the list; click any attribute to filter or search instantly.

### Appearance & Customisation
- **7 built-in themes** — Windows Dark/Light, Studio Dark/Light, Amber Console, Frost, Tokyo Night.
- **3 UI languages** — English, 简体中文, 日本語; UI scale adjustable.
- **Customisable columns** — 12 columns, drag to resize, right-click header or use settings to show/hide.

---

## Keyboard Shortcuts

| Shortcut | Action |
| --- | --- |
| `Space` | Pause / Resume |
| `Alt + A` | Toggle Auto Play |
| `L` | Toggle loop |
| `↑` / `↓` / `PageUp` / `PageDown` / `Home` / `End` | Navigate list |
| `Ctrl + A` | Select all matching results |
| `Ctrl + Z` | Undo last favorite or category edit |
| `Ctrl + Shift + Z` / `Ctrl + Y` | Redo |
| `Ctrl + O` | Add sample library |
| `Ctrl + ,` | Open settings |
| `Esc` | Clear waveform region / close menu or dialog |

---

## Supported Formats

- **Audio:** `.wav` `.aif` `.aiff` `.flac` `.mp3` `.ogg` `.m4a` `.aac` `.opus`
- **Metadata & artwork:** ID3, Vorbis Comments, M4A Atoms, BWF `bext`, iXML, RIFF `LIST INFO`; embedded and sidecar cover images

---

## Building from Source

### Requirements
- Windows 10 / 11 (64-bit)
- Rust (Edition 2024) and Node.js / npm
- For ASIO support: Steinberg ASIO SDK 2.3 (extract to `%LOCALAPPDATA%\mio\asio_sdk`) and `libclang.dll` (set `LIBCLANG_PATH`)

### Build Commands

```bash
# Development build (WASAPI only, no ASIO SDK needed)
./tools/build-app.sh --no-asio

# Full build (WASAPI + ASIO)
./tools/build-app.sh

# Package Windows installer
./ui/node_modules/.bin/tauri build --features asio --bundles nsis
```

---

## License

This project is licensed under **GNU AGPL v3** (`AGPL-3.0-or-later`), see [LICENSE](LICENSE).  
Third-party component notices: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
