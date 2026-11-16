# AGENTS.md

ReSlip: a sample manager backend built around two hard requirements — indexing a large library fast, and starting playback of any file with latency low enough to feel instant. Rust core, Tauri shell.

Status: **设计阶段。架构已定，未开始实现。** 决策与依据见 `docs/architecture.md`；调研结论见 `docs/TODO-low-latency-audio.md`；实测数据见 `docs/research-notes-playback.md`。

## Stack (decided)

- Rust workspace, edition 2024
- Tauri 2 shell (WebView2 on Windows)
- `symphonia` for decoding (`all-formats` + `all-codecs`)
- `wasapi` crate 0.27 直接对接 Windows 音频（**不用 cpal**：需要 `IAudioClient3` 周期查询与 `get_current_padding` 排队量，cpal 拿不到）
- 输出后端：**WASAPI 共享模式为默认**（实测 17.5ms 点击到出声，不抢设备）；ASIO 为可选 feature（延迟最低但 SDK 专有许可 + 构建需 libclang）；**WASAPI 独占否决**（实测会让用户开着的 DAW 静音）
- SQLite 索引：`rusqlite` bundled + WAL + FTS5
- ffmpeg 保留为外部进程，只用于导出与特效渲染；播放路径不经过 ffmpeg
- 只做 Windows（不做 macOS/Linux）

## Rules

- **Latency is a measured number, not an adjective.** Any claim about click-to-sound must state which measurement it is: `publish → 回调写入` and `publish → 出声` differ by 12ms on shared mode. Numbers must come with the machine's audio stack (driver, sample rate, buffer period). The numbers in `docs/research-notes-playback.md` were measured on the reference machine and must be re-verified before being used as targets.
- **No placeholder engines.** Do not ship a stub audio path "to be optimized later"; the whole point of this project is the engine.
- **The UI/UX is inherited.** Slip Sound's interaction design is considered good and is the reference; this project replaces the backend, not the interaction model. Read the Slip Sound repo's `AGENTS.md` and `src/renderer/src/components/WAVEFORM-PLAYBACK.md` for the behaviour that must be preserved.
- Indexing and playback are separate subsystems with separate acceptance numbers. Do not let one block the other.
- **The renderer never touches PCM.** Waveform peaks come from the backend as ~1000 floats; decoded audio never crosses IPC into the webview.
- **The audio callback does no allocation, no locking, no IO, no logging.** It copies from a pre-decoded slot and nothing else.

## Where things live

| Area | Path |
| --- | --- |
| 架构决策（后端选型、线程模型、技术栈、实施顺序） | `docs/architecture.md` |
| 调研结论（Q1–Q6 逐条答复） | `docs/TODO-low-latency-audio.md` |
| 实测数据（延迟、索引、格式探测） | `docs/research-notes-playback.md` |
| 可复现的验证程序 | `probes/`（用法见 `probes/README.md`） |
| 开工提示词（复制到新对话） | `docs/prompt-for-implementation.md` |
| Handoff log | `docs/agent-log.md` |
