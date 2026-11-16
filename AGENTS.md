# AGENTS.md

ReSlip: a sample manager backend built around two hard requirements — indexing a large library fast, and starting playback of any file with latency low enough to feel instant. Rust core, Tauri shell.

Status: **P0（拖拽导出）、P1（音频引擎原型）、P2（索引器 + schema）已通过。** 产品代码在 `crates/`。决策与依据见 `docs/architecture.md`；引擎与索引的模块细节见 `docs/engine.md`、`docs/index.md`；调研结论见 `docs/TODO-low-latency-audio.md`；实测数据见 `docs/research-notes-playback.md`；进度与遗留见 `docs/agent-log.md`。

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
- **Gapless decoding is mandatory, not an optimisation.** With `FormatOptions::enable_gapless` off, MP3 keeps 25.1ms of LAME encoder delay at the head; MP4/AAC keeps 1024 priming frames (23.2ms) because symphonia's isomp4 parses `elst` but never surfaces it. Both are the same order of magnitude as the entire device path, so a missing trim is felt as a late trigger.

## Commands

```bash
cargo build --release          # workspace, release profile
cargo test                     # engine + index unit tests
cargo run --release -p reslip-cli -- devices
cargo run --release -p reslip-cli -- play <file> --device E2x2
cargo run --release -p reslip-cli -- switch <f1> <f2> ... --device E2x2 --interval 25 --rounds 5
cargo run --release -p reslip-cli -- probe <file>                          # decode cost only
cargo run --release -p reslip-cli -- render <in> <out.wav> --device E2x2   # PCM path check
cargo run --release -p reslip-cli -- index <folder>                        # incremental pass
cargo run --release -p reslip-cli -- index <folder> --full                 # re-probe + prune
cargo run --release -p reslip-cli -- index-stats <folder>
cargo run --release -p reslip-cli -- search <folder> <query> --limit 25
cargo run --release -p reslip-cli -- classify-diff <folder> <reference.db> # parity vs Slip Sound
cargo run --release -p reslip-cli -- probe-bench <folder> --limit 20000    # per-format probe cost
```

`--device` matches a friendly-name substring. The default device is FxSound (a virtual card), so latency measured without `--device E2x2` means nothing.

## Where things live

| Area | Path |
| --- | --- |
| 音频引擎（解码、重采样、双槽、WASAPI 输出） | `crates/reslip-engine/` |
| 索引器（头探测、分类、schema、搜索、文件夹树） | `crates/reslip-index/` |
| 命令行验收工具（P1 / P2 的验收产物） | `crates/reslip-cli/` |
| 架构决策（后端选型、线程模型、技术栈、实施顺序） | `docs/architecture.md` |
| 引擎模块细节 | `docs/engine.md` |
| 索引模块细节 | `docs/index.md` |
| 调研结论（Q1–Q6 逐条答复） | `docs/TODO-low-latency-audio.md` |
| 实测数据（延迟、索引、格式探测） | `docs/research-notes-playback.md` |
| 可复现的验证程序 | `probes/`（用法见 `probes/README.md`） |
| 分类词典生成器（勿手改 `dictionary.rs`） | `tools/gen-category-dict.mjs` |
| 开工提示词（复制到新对话） | `docs/prompt-for-implementation.md` |
| Handoff log | `docs/agent-log.md` |
