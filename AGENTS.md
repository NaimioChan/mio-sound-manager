# AGENTS.md

Mio Sound Manager: a sample manager backend built around two hard requirements — indexing a large library fast, and starting playback of any file with latency low enough to feel instant. Rust core, Tauri shell.

Status: **P0（拖拽导出）、P1（音频引擎原型）、P2（索引器 + schema）、P3（Tauri 外壳 + IPC）、P4（前端搬迁）、P5（工作区：多库 / 持久化 / undo / 阻塞索引 / 设置）、P6（ASIO 后端 + 设备选择）、P7（改名 Mio Sound Manager + 西门子 iX 重写前端 + 主题系统 + 输出音量）已通过。P8 及之后不做。** 产品代码在 `crates/`。决策与依据见 `docs/architecture.md`；引擎、索引、外壳的模块细节见 `docs/engine.md`、`docs/index.md`、`docs/app.md`；进度与遗留见 `docs/agent-log.md`。调研阶段的原始测量笔记与开工提示词已移除，需要复测时用 `probes/` 重跑。

## Stack (decided)

- Rust workspace, edition 2024
- Tauri 2 shell (WebView2 on Windows)
- `symphonia` for decoding (`all-formats` + `all-codecs`)
- `wasapi` crate 0.27 直接对接 Windows 音频（**不用 cpal**：需要 `IAudioClient3` 周期查询与 `get_current_padding` 排队量，cpal 拿不到）
- 输出后端：**WASAPI 共享模式为默认**（实测 17.5ms 点击到出声，不抢设备）；**ASIO 为可选 feature 并已在设置里可选**（实测 64 帧下 5.31ms 点击到出声，SDK 专有许可 + 构建需 libclang）；**WASAPI 独占否决**（实测会让用户开着的 DAW 静音）
- 前端：**SolidJS + `@siemens/ix` 5.3 web components**（西门子设计系统）。颜色一律走 `--theme-*` token，不写字面色值；主题系统见 `ui/src/theme.ts`
- SQLite 索引：`rusqlite` bundled + WAL + FTS5
- ffmpeg 保留为外部进程，只用于导出与特效渲染；播放路径不经过 ffmpeg
- 只做 Windows（不做 macOS/Linux）

## Rules

- **Latency is a measured number, not an adjective.** Any claim about click-to-sound must state which measurement it is: `publish → 回调写入` and `publish → 出声` differ by 12ms on shared mode. Numbers must come with the machine's audio stack (driver, sample rate, buffer period). The figures in `docs/architecture.md` sections 2, 3 and 8.1 were measured on the reference machine and must be re-verified with `probes/` before being used as targets.
- **No placeholder engines.** Do not ship a stub audio path "to be optimized later"; the whole point of this project is the engine.
- **ASIO 驱动要在 STA 线程上加载。** 驱动对象是 `CoCreateInstance` 造的，TOE 驱动只从单线程套间交出对象：MTA 工作线程会得到 `loadDriver` 返回 false，STA 工作线程成功。WASAPI 要 MTA，所以两个后端各自一个渲染线程，不能共用。
- **The UI/UX is inherited.** Slip Sound's interaction design is considered good and is the reference; this project replaces the backend, not the interaction model. Read the Slip Sound repo's `AGENTS.md` and `src/renderer/src/components/WAVEFORM-PLAYBACK.md` for the behaviour that must be preserved.
- Indexing and playback are separate subsystems with separate acceptance numbers. Do not let one block the other.
- **The renderer never touches PCM.** Waveform peaks come from the backend as ~1000 floats; decoded audio never crosses IPC into the webview.
- **The audio callback does no allocation, no locking, no IO, no logging.** It copies from a pre-decoded slot and nothing else.
- **Colours come from iX tokens, never from literals.** `ui/src/styles.css` holds geometry only; every colour is a `--theme-*` variable so the theme system repaints the iX components, our CSS and the waveform canvas from one override block. A literal hex in a component is a colour that will not follow the theme.
- **The theme has one owner.** `ui/src/theme.ts` writes `mio:theme`; it must not also live in the settings blob — two writers made a reload fall back to the default.
- **Volume is engine state, not renderer state.** `set_gain` writes one atomic the render callback reads; the slider's signal is a mirror. A volume control that only updated the UI would pass a DOM check and do nothing.
- **Gapless decoding is mandatory, not an optimisation.** With `FormatOptions::enable_gapless` off, MP3 keeps 25.1ms of LAME encoder delay at the head; MP4/AAC keeps 1024 priming frames (23.2ms) because symphonia's isomp4 parses `elst` but never surfaces it. Both are the same order of magnitude as the entire device path, so a missing trim is felt as a late trigger.

## Commands

```bash
cargo build --release          # workspace, release profile (engine + index + cli)
cargo build --release --features asio   # 同上 + ASIO 后端（需 ASIO SDK 与 libclang）
cargo test --release           # all unit + integration tests
cargo run --release -p mio-cli -- devices            # 两个后端的设备一起列
cargo run --release -p mio-cli -- play <file> --device E2x2
cargo run --release -p mio-cli -- play <file> --backend asio --device TOPPING --asio-frames 64
cargo run --release -p mio-cli -- switch <f1> <f2> ... --device E2x2 --interval 25 --rounds 5
cargo run --release -p mio-cli -- probe <file>                          # decode cost only
cargo run --release -p mio-cli -- render <in> <out.wav> --device E2x2   # PCM path check
cargo run --release -p mio-cli -- index <folder>                        # incremental pass
cargo run --release -p mio-cli -- index <folder> --full                 # re-probe + prune
cargo run --release -p mio-cli -- index-stats <folder>
cargo run --release -p mio-cli -- search <folder> <query> --limit 25
cargo run --release -p mio-cli -- classify-diff <folder> <reference.db> # parity vs Slip Sound
cargo run --release -p mio-cli -- probe-bench <folder> --limit 20000    # per-format probe cost

# The desktop shell. Two steps: bundle the webview, then the exe embeds it.
(cd ui && npm install && npm run build)
cargo build -p mio-app --release
./tools/build-app.sh --asio             # 同上，加 ASIO（检查 SDK 与 libclang 后带 feature 构建）
./tools/run-app.sh                      # launches with the webview debugger on :9223
./tools/kill-app.sh                     # Windows won't replace a mapped exe
```

`--device` matches a friendly-name substring. The default device is FxSound (a virtual card), so latency measured without `--device E2x2` means nothing. `--backend asio` switches to the ASIO driver path (build with `--features asio` first); ASIO has no system default, so an omitted `--device` takes the first installed driver.

The shell is not a default workspace member: `cargo build`/`cargo test` at the root must keep working without an npm step. Build it with `-p mio-app` after bundling `ui/`.

**The shell is verified over CDP, not by looking at it.** `tools/run-app.sh` sets `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` before the webview exists; the scripts under `tools/` then drive the window and assert on the real DOM:

```bash
node tools/check-interactions.mjs   # 列表/选择/排序/搜索/菜单/modal/AUTO PLAY/播放头/seek/设置
node tools/check-themes.mjs         # 六套主题应用、重载保留、WCAG 对比度
node tools/check-volume.mjs         # 滑杆 → IPC → 引擎 → 回读，静音，重载
node tools/check-layout.mjs         # DOM 几何：区域拼合、列对齐、dock 控件顺序、无溢出
node tools/shot.mjs [out.png]       # 截图（给人看）
node tools/shot-themes.mjs [dir]    # 每套主题一张截图
```

They resolve `playwright-core` from `slip-sound/package.json` through `createRequire`, so they run from any directory — that module is not a dependency of this repo. See `docs/app.md`.

## Where things live

| Area | Path |
| --- | --- |
| 音频引擎（解码、重采样、双槽、WASAPI / ASIO 输出、波形峰值） | `crates/mio-engine/` |
| 索引器（头探测、分类、schema、搜索、文件夹树） | `crates/mio-index/` |
| Tauri 外壳（IPC 命令、应用状态、库生命周期） | `crates/mio-app/` |
| 前端（SolidJS + Vite + @siemens/ix，构建产物编进 exe） | `ui/` |
| 配色主题系统（八色调色板 → iX token 推导） | `ui/src/theme.ts` |
| 命令行验收工具（P1 / P2 的验收产物） | `crates/mio-cli/` |
| 架构决策（后端选型、线程模型、技术栈、实施顺序） | `docs/architecture.md` |
| 引擎模块细节 | `docs/engine.md` |
| 索引模块细节 | `docs/index.md` |
| 外壳与 IPC 细节 | `docs/app.md` |
| 可复现的验证程序 | `probes/`（用法见 `probes/README.md`） |
| 外壳的 CDP 驱动与检查脚本 | `tools/` |
| 分类词表（改这里，再跑生成器） | `tools/taxonomy.py` |
| 分类词典生成器（勿手改 `dictionary.rs`） | `tools/gen-category-dict.py` |
| Handoff log | `docs/agent-log.md` |
