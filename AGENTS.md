# AGENTS.md

ReSlip: a sample manager backend built around two hard requirements — indexing a large library fast, and starting playback of any file with latency low enough to feel instant. Rust core, Tauri shell.

Status: **调研阶段 (research phase). No implementation yet.** The open questions and their acceptance criteria live in `docs/TODO-low-latency-audio.md`. Do not start writing the audio engine until that document's decisions are settled.

## Stack (decided)

- Rust workspace, edition 2024
- Tauri 2 shell (WebView2 on Windows)
- `symphonia` for decoding, `cpal` for output (ASIO reachable through cpal's `asio` feature)
- SQLite for the index (`rusqlite` with bundled SQLite, or `sqlite3` via `libsqlite3-sys`)

## Stack (under investigation)

- Output backend: WASAPI shared vs WASAPI exclusive vs ASIO. See the TODO doc; do not commit to one yet.
- Decode strategy: full-file decode vs streaming vs decode-ahead window.
- Whether any DSP/format work happens in Rust vs in the UI.

## Rules

- **Latency is a measured number, not an adjective.** Any claim about click-to-sound must come with a reproducible measurement and the machine's audio stack noted (driver, sample rate, buffer period). The numbers in `docs/research-notes-playback.md` were measured on the reference machine and must be re-verified before being used as targets.
- **No placeholder engines.** Do not ship a stub audio path "to be optimized later"; the whole point of this project is the engine.
- **The UI/UX is inherited.** Slip Sound's interaction design is considered good and is the reference; this project replaces the backend, not the interaction model. Read the Slip Sound repo's `AGENTS.md` and `src/renderer/src/components/WAVEFORM-PLAYBACK.md` for the behaviour that must be preserved.
- Indexing and playback are separate subsystems with separate acceptance numbers. Do not let one block the other.

## Where things live

| Area | Path |
| --- | --- |
| Research backlog and open decisions | `docs/TODO-low-latency-audio.md` |
| Measurements captured so far | `docs/research-notes-playback.md` |
| Handoff log | `docs/agent-log.md` |
