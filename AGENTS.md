# AGENTS.md

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
cargo run --release -p mio-cli -- classify-check <expect.json>          # Rust 与 Python 镜像对等
cargo run --release -p mio-cli -- probe-bench <folder> --limit 20000    # per-format probe cost

# The desktop shell. Two steps: bundle the webview, then the exe embeds it.
(cd ui && npm install && npm run build)
cargo build -p mio-app --release
./tools/build-app.sh                     # 产品构建：WASAPI + ASIO（检查 SDK 与 libclang 后带 feature 构建）
./tools/build-app.sh --no-asio           # 纯 WASAPI 的开发构建，不需要 ASIO SDK
./ui/node_modules/.bin/tauri build --features asio --bundles nsis   # 安装包（仓库根执行，先 kill-app）
./tools/package-portable.sh              # 便携版 zip（0.1.2 起不再发，脚本留着重建旧包）
./tools/run-app.sh                      # launches with the webview debugger on :9223
./tools/kill-app.sh                     # Windows won't replace a mapped exe
```

`--device` matches a friendly-name substring. The default device is FxSound (a virtual card), so latency measured without `--device E2x2` means nothing. `--backend asio` switches to the ASIO driver path (build with `--features asio` first); ASIO has no system default, so an omitted `--device` takes the first installed driver.

The shell is not a default workspace member: `cargo build`/`cargo test` at the root must keep working without an npm step. Build it with `-p mio-app` after bundling `ui/`.

**The shell is verified over CDP, not by looking at it.** `tools/run-app.sh` sets `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` before the webview exists; the scripts under `tools/` then drive the window and assert on the real DOM:

```bash
node tools/check-interactions.mjs   # 列表/选择/排序/搜索/菜单/modal/AUTO PLAY/播放头/seek/设置
node tools/check-themes.mjs         # 七套主题应用、重载保留、WCAG 对比度（每套都测）
node tools/check-volume.mjs         # 滑杆 → IPC → 引擎 → 回读，静音记忆，重载
node tools/check-region.mjs         # 行可拖、框选、循环跟随框选、区域导出 WAV、Esc 清除（跑完要重启）
node tools/check-layout.mjs         # DOM 几何：区域拼合、列对齐、dock 控件顺序、无溢出
node tools/check-i18n.mjs           # 三语字典：键集齐平、占位符不丢不多
node tools/check-tags.mjs           # 侧栏模式下拉/旧键迁移、列宽拖动与显隐、表头菜单、横向滚动、库头无三角、BPM 列、tag 多选、无副标题
node tools/check-favorite.mjs       # 星标不重查（epoch/骨架/searching）、字形延迟、侧栏计数、undo、收藏过滤下必须重查
node tools/check-stress.mjs         # 压力测试：仅在前后端数据结构/算法有破坏性改动时运行
node tools/check-shipped.mjs [port] # 对着打包产物验版本/ASIO/设备/新控件（解压 zip 后起那个 exe 再跑）
node tools/shot.mjs [out.png]       # 截图（给人看）
node tools/shot-themes.mjs [dir]    # 每套主题一张截图
```

They resolve `playwright-core` from `slip-sound/package.json` through `createRequire`, so they run from any directory — that module is not a dependency of this repo. See `docs/app.md`.

The seven window-driving `check-*.mjs` assert on English copy and pin `mio:locale` to `en` before reloading — on a Chinese or Japanese machine they would otherwise fail on a missing locator rather than a broken feature. Column headers and menu items are also addressable by `data-sort-key` / `data-menu-id`, so a selector never depends on translated text. Copy coverage belongs to `check-i18n.mjs`; run `check-stress.mjs` only for destructive data-structure or algorithm changes.

**`check-region.mjs` 会进入原生 OLE 拖拽循环，跑完 webview 不再响应 IPC**：它必须单独跑，之后要 `kill-app.sh` 再起窗口。详情见 `tools/README.md`。

## Stack (decided)

- Rust workspace, edition 2024
- Tauri 2 shell (WebView2 on Windows)
- `symphonia` for decoding (`all-formats` + `all-codecs`)
- `wasapi` crate 0.27 直接对接 Windows 音频（**不用 cpal**：需要 `IAudioClient3` 周期查询与 `get_current_padding` 排队量，cpal 拿不到）
- 输出后端：**WASAPI 共享模式为默认**（实测 17.5ms 点击到出声，不抢设备）；**ASIO 为可选 feature 并已在设置里可选**（实测 64 帧下 5.31ms 点击到出声，构建需 libclang，SDK 按 GPLv3 一支随产物分发）；**WASAPI 独占否决**（实测会让用户开着的 DAW 静音）
- 前端：**SolidJS + `@siemens/ix` 5.3 web components**（西门子设计系统）。颜色一律走 `--theme-*` token，不写字面色值；主题系统见 `ui/src/theme.ts`
- 多语言：**中 / 英 / 日**，`ui/src/i18n.ts` 加 `ui/src/locales/{en,zh,ja}.ts`。英文是键集基准，另两语类型化为 `Record<MessageKey, string>`，漏键即编译错误。只翻外壳文案，不翻数据（分类名、文件名）
- 图标：**SVG 复制进仓库** `ui/src/assets/icons/`，`?raw` 导入拼 data URL 交给 `ix-icon`。不按名字取（`ix-icon` 拿名字会去 fetch `svg/<name>.svg`，产物里没有该目录）；不用 `encodeURIComponent`（iX 只调 `decodeURI`，`%3D` 不还原会让 SVG 解析成空框）。离线可用是硬要求
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
- **列状态只有一个拥有者：`ui/src/columns.ts`。** `SortKey` / `COLUMNS` / `SORT_WIRE` / `DEFAULT_DIR` 与顺序、宽度、显隐都从那里读；`SoundTable` 不再导出它们，设置页和表头菜单也不另存一份。表头、骨架行、真实行必须继续用同一个 `gridTemplate()`，轨道全是像素（`fr` 会让拖出来的宽度随窗口变）。三个信号整体替换，原地改 `Set`/对象不会触发 Solid 通知。
- **iX 组件上只用 `classList`，不要用 `class`。** Solid 把 `class={…}` 写成 `className` 的整体赋值，会抹掉 Stencil 的 `hydrated` 与 `sc-*` 类；iX 对 `:not(.hydrated)` 的 host 是 `visibility:hidden`，症状是「点一下就消失」而不是样式错。
- **新增 `<ix-*>` 元素要同步 `ui/vite.config.ts` 的 `IX_TAGS`。** 漏了会被 `trimIxChunks` 裁成 `data:text/javascript,`，CSP 拒绝后元素永不升级——症状是一整块空白区域，控制台只有一行 `Constructor for "ix-x#undefined" was not found`。
- **Nothing user-facing is written as a literal string.** Every label goes through `t()`; the backend sends *kinds* (`EditKind`), never English prose for the renderer to display — a string born in Rust can never be translated. Category names, file names and sample rates are data and stay untranslated; `Uncategorized` is the one exception, because it is a schema sentinel rather than a classifier guess.
- **No subtitles.** Nothing under a heading, a brand, a modal title or a settings group. 标题本身就是完整信息；副标题是不承载信息的一行小字，只会把版式撑高。文案层面同理：不要为标题补一句解释性的第二行。要解释就写进正文或 `helperText`，那不是 subtitle。
- **Gapless decoding is mandatory, not an optimisation.** With `FormatOptions::enable_gapless` off, MP3 keeps 25.1ms of LAME encoder delay at the head; MP4/AAC keeps 1024 priming frames (23.2ms) because symphonia's isomp4 parses `elst` but never surfaces it. Both are the same order of magnitude as the entire device path, so a missing trim is felt as a late trigger.
- **完整 UCS A 先在 CatID 层打分，再投影到 22 类。** native UCS 字段保留官方 CatID/category/subcategory；旧的 22 类字段由 CatID/词汇投影派生。不要恢复 curated 优先、UCS 仅兜底的旧两遍结构；证据筛选仍是硬要求。
- **UCS 的同义词必须过证据筛，不能按看起来对不对收。** `tools/ucs-evidence.py` 量的是「文件名含该词、且本项目已有答案的文件里，答案与 UCS 映射一致率」。`trance`（曲风 vs MAGIC-SPELL）、`hybrid`（12,454 个文件/19 个包的包名）、`calendar`、`acid` 都是看起来合理、在库里全错的词。
- **Python 镜像给出的数字要先过 `mio-cli classify-check` 才能引用。** 镜像会漂移，本轮漂了两次都是这条检查抓出来的；0 mismatches 之前，`tools/classify-mirror.py` 的输出只是关于一个不存在的分类器的测量。

## Where things live

| Area | Path |
| --- | --- |
| 音频引擎（解码、重采样、双槽、WASAPI / ASIO 输出、波形峰值） | `crates/mio-engine/` |
| 索引器（头探测、分类、schema、搜索、文件夹树） | `crates/mio-index/` |
| Tauri 外壳（IPC 命令、应用状态、库生命周期） | `crates/mio-app/` |
| 前端（SolidJS + Vite + @siemens/ix，构建产物编进 exe） | `ui/` |
| 列定义、顺序、宽度、显隐（唯一拥有者） | `ui/src/columns.ts` |
| 配色主题系统（八色调色板 → iX token 推导） | `ui/src/theme.ts` |
| 多语言（语言信号 + `t()`，三语字典） | `ui/src/i18n.ts`、`ui/src/locales/` |
| 图标资源（从 `@siemens/ix-icons` 复制，离线可用） | `ui/src/assets/icons/` |
| 品牌标记（顶栏 `<img>`）与应用图标源（本机 `logo/`，已 gitignore；产物在 `crates/mio-app/icons/`） | `ui/src/assets/brand.svg`、`tools/make-app-icon.py` |
| 命令行验收工具（P1 / P2 的验收产物） | `crates/mio-cli/` |
| 架构决策（后端选型、线程模型、技术栈、实施顺序） | `docs/architecture.md` |
| 引擎模块细节 | `docs/engine.md` |
| 索引模块细节 | `docs/index.md` |
| 外壳与 IPC 细节 | `docs/app.md` |
| 可复现的验证程序 | `probes/`（用法见 `probes/README.md`） |
| 外壳的 CDP 驱动与检查脚本 | `tools/` |
| 发版（安装包、产物验收） | `tools/README.md`「发版」一节 |
| BPM 解析 | `crates/mio-index/src/bpm.rs` |
| 分类词表（改这里，再跑生成器） | `tools/taxonomy.py` |
| 分类词典生成器（勿手改 `dictionary.rs` / `ucs.rs`） | `tools/gen-category-dict.py` |
| UCS 兜底词表：映射、导入、证据、报告 | `tools/ucs_mapping.py`、`tools/import-ucs.py`、`tools/ucs-evidence.py`、`tools/ucs-report.py` |
| 分类器的 Python 镜像（测速用，先过 `mio-cli classify-check`） | `tools/classify-mirror.py` |
| Handoff log（进度与遗留，AGENTS.md 不记进度） | `docs/agent-log.md` |
