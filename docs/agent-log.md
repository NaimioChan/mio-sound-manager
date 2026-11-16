# Agent log

Shared handoff log. Newest entry on top. One entry per work session: what changed, what was verified, what is left.

## 2026-11-16 15:10 · hermes · P3 Tauri 外壳 + IPC 落地

- 改: 新建 `crates/reslip-app`（`commands` / `state` / `library` / `error` + `tests/ipc.rs`）、`ui/`（SolidJS + Vite，`App` / `SoundTable` / `WaveformDock` / `Sidebar` / `ipc.ts`）、`tools/`（`run-app.sh` / `kill-app.sh` / `inspect-app.mjs` / `drive-app.mjs` / `drive-big-library.mjs` / `check-interactions.mjs`）；`crates/reslip-engine` 新增 `waveform` 模块，`Engine` 加 `publish_from` / `stop` / `monitor` / `PlaybackMonitor`；新增 `docs/app.md`；更新 `AGENTS.md`、`docs/architecture.md` 第 8 节
- 状态: **P3 通过**，产品代码 = engine + index + app + ui
- 验收库: `D:/reslip-test-library`（28 万文件），设备 FxSound（默认设备，48k 2ch f32，引擎周期 11.00ms）

### 验收怎么测的（不靠人看窗口）

`tests/ipc.rs` 15 个测试直接驱动 `AppState`，跑真实设备与真实索引，不打开窗口。加上 `commands.rs` 的 7 个与 P1/P2 的 57 个，`cargo test --release --workspace` 共 **79 个通过**。

窗口内的交互用 CDP 驱动真实窗口断言：

- `tools/check-interactions.mjs`: 播放头推进（0.61 → 1.12s）、点波形 75% 处 seek 到 94.68s（目标 94.06s）、Stop 后播放头钉在 0.00s 不动
- `tools/drive-big-library.mjs`（真实 28 万库）:

| 口径 | 实测 |
| --- | --- |
| `open_library` → 首屏有行 | **69 ms** |
| 搜索 `gunshot` | 38 行 / 约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| 增量索引全程 | **6.5 s**（P2 命令行 5.4s，差异是这里顺序走完后 UI 轮询结束） |

索引期间 UI 仍可查询（WAL），侧栏与状态栏的数字实时增长。

### 引擎的三处改动及理由

1. **`publish` / `publish_from` / `stop` 改 `&self`。** 它们只写原子量与 `arc-swap`，不需要 `&mut`。这样任何线程都能发布，命令层不必再搭控制线程串行化。
2. **`publish_from(clip, offset_frames)`。** IPC 的 `start_at` 落到这里；seek 是在不可变缓冲上换起始位置，代价与普通播放相同的一次原子写，不重新解码。
3. **`PlaybackMonitor`。** `Engine::monitor()` 给 webview 一份只读句柄，`snapshot()` 与 `drain_reports()` 都是纯原子读 / 无锁队列，播放头轮询不碰音频线程的槽。

波形在 `decode_source`（原始采样率交错 f32，不重采样不打包）之上做 `bucket_peaks`。**桶边界按帧精确，不是参考实现的 256 帧块量化**：2 秒文件的 1000 桶视图上，块量化会让每个桶边界最多偏 5.8ms，尖锐瞬态会画到隔壁柱子。峰值来自播放用的同一次解码（含 gapless 裁剪），否则波形与声音差 23–25ms。

### 踩到的两个坑

1. **`tauri` 必须开 `custom-protocol`。** 不开时 release 构建仍把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是空白窗口。第一次运行正是如此：CDP 里页面 URL 是 `http://localhost:1420/`，改成 `http://tauri.localhost/` 后才从 exe 内部取 asset。
2. **Windows 不允许替换正在映射的 exe**，`cargo build --release` 报 `failed to remove file reslip.exe`。`tools/kill-app.sh` 用 PowerShell `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

另有一条工程约定：**要想让 UI 可被验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。驱动脚本放在 `slip-sound/` 下跑——`playwright-core` 装在那里，且 ESM 按脚本自身位置解析 `node_modules`。

- 验证: `cargo test --release --workspace` 79 个通过；`ui/` 的 `tsc --noEmit` 与 `vite build` 通过（26KB JS）；真实 28 万库的打开/搜索/点击/波形/seek/stop 全部实测；截图见 `reslip-big-playing.png`
- 遗留: ① 打开库用 `window.prompt` 而非原生文件夹对话框（需 `tauri-plugin-dialog` + ACL，P4 一起做）；② 侧栏只有文件夹树，无 category/subcategory facet 计数与收藏行（`reslip-index` 的 `facets()` 已就绪，缺 P4 的交互规格）；③ P0 验证过的拖拽导出还没接到列表；④ 键盘操作（方向键选行、空格播放）属于 P4；⑤ 长文件仍整段解码，波形因此也是一次整段解码，`decodeMs` 直接显示给用户（实测短素材 0.9–9.4ms）；⑥ `autoAdvance`（播完自动下一行）未做，参考实现有
- 下一步: P4 前端搬迁（列表/波形/搜索的完整交互：键盘、多选、拖拽导出、原生目录选择、分类侧栏）

## 2026-11-16 14:40 · hermes · P2 索引器 + schema 落地

- 改: 新建 `crates/reslip-index`（probe / wav_header / classify / db / folders / indexer）；`crates/reslip-cli` 新增 `index` / `index-stats` / `search` / `classify-diff` / `probe-bench` 五个子命令；新增 `tools/gen-category-dict.mjs`（词典生成器）与 `probes/wav-fmt20`（缺陷复现）；新增 `docs/index.md`；本条日志
- 状态: **P2 通过**，产品代码 = engine + index + cli
- 验证库: `D:\reslip-test-library`，280,805 个文件的硬链接镜像（原库 `SoundLibrary` 被 Slip Sound 应用占用，未动）

### 分类一致性：271,350 行零差异

`reslip-cli classify-diff` 用 Rust 分类器重新分类旧库每条路径，逐字段比对 category / subcategory / confidence / matched_terms：**四项全部 0 差异**。

这个检查抓到两个真实移植缺陷，都是不测就会静默错下去的：

1. **同分裁决依赖插入顺序。** 参考实现遍历 `Object.entries(scores)`（插入顺序），我用 `HashMap`（随机顺序），同级子分类打平时选中项不同 → 3,932 行归错分类。改用 `Vec<(String, u32)>` 保持插入顺序。
2. **`matched_terms` 未全局去重。** 参考是 `[...new Set()]`，我用了 `Vec::dedup`（只去相邻重复），短语与单词同时命中时留下 `["magic","spell","magic","spell","magic","spell"]`。

两条都补了回归测试，注释写明实测来源。

### 实测（本机，D 盘 Kingston SNV2S2000G，无 DRAM 的入门 SSD）

| 场景 | 文件数 | 耗时 | 吞吐 |
| --- | --- | --- | --- |
| 全量首次 | 271,350 | **63.9s** | 4,247 文件/s |
| 增量（无变化） | 271,350 | **5.4s** | 探测 0 个文件 |

分阶段（全量）：

| 阶段 | 耗时 | 说明 |
| --- | --- | --- |
| 遍历 | 4.98s | 单线程目录扫描 |
| 探测（16 线程） | 32.5s | 每文件 1.9ms，随机读延迟受限 |
| 分类 | 6.24s | 每文件 0.023ms |
| 写入 | 7.04s | 批量事务，索引后建 |
| prune | 2.94s | `NOT EXISTS` 命中主键 |
| FTS 重建 | 10.18s | 一次性 rebuild |

**与架构文档 5.1 的 9s 外推不符，且不能靠调优弥合。** 那个数字外推自 2 万文件合成库（预生成文件 + NVMe），这里是 28 万真实文件在无 DRAM SSD 上。探测阶段的 `stat + open + 读 64KB` 累计占 80%，解析只占 20%——瓶颈是随机读，不是解析代码。同一份代码在热缓存小样本上跑 10,790 文件/s，在冷缓存全库上 4,247 文件/s，差距就是磁盘。

优化过的三处，都有实测依据：

- **重复 stat 去掉两次**：`walkdir` 返回的 `DirEntry::metadata()` 在 Windows 上取自 `FindNextFileW`，不额外系统调用。之前遍历、探测、增量检查各 stat 一次，8.2s 浪费在这里。
- **线程上限从 8 放开到核数**：8 → 16 线程还有 22% 收益（4,347 → 5,331 文件/s），因为探测是延迟受限而非 CPU 受限。
- **批量装载卸掉 FTS 触发器与二级索引**：写入阶段 16.9s → 6–7s；prune 的 `NOT IN` 子查询 36s → 3s（改 `NOT EXISTS`）。

### 修掉的两个真实缺陷

1. **WAV 的 20 字节 `fmt ` chunk 被 symphonia 拒绝**。`symphonia-format-riff` 的 `read_pcm_fmt` 只接受 16 / 18 / 40，其它长度报 `malformed fmt_pcm chunk`。20 字节是合法的 `WAVEFORMATEX`（`cbSize = 2`），真实库里有 284 个，全部是可正常播放的 PCM。新增 `wav_header.rs` 做 RIFF 头兜底解析（只读头，不碰音频数据）。复现程序在 `probes/wav-fmt20`。
2. **路径分隔符混用导致 prune 删光旧行**。`walkdir` 返回的路径继承传入根的分隔符，根传 `D:/lib` 会得到 `D:/lib\sub\a.wav`。混合形式破坏唯一键，一次全量扫描把旧库 271,350 行全部 prune 掉。修法是在入口统一归一化（`normalize_root` / `store_path`），文件夹树与文件夹区间查询对根做同样处理。

另外过滤了 macOS 的 `._*` 资源叉文件（9,455 个，带音频扩展名的 AppleDouble 元数据）。过滤后元数据为 null 的行从 9,739 降到 224（0.08%），剩下的都是内容非音频的 `.wav`。

- 验证: `cargo test --release` 44 个单测通过（probe 兜底、WAV 头解析、分类插入顺序与去重、upsert 保留用户状态、prune、FTS 操作符注入、文件夹区间、增量与全量）；真实库 271,350 行分类零差异；搜索 "gunshot" 208 命中 / 2.0ms、"rain" 611 命中 / 5.2ms；文件夹树 Bought 13,875 + UnBought 257,475
- 遗留: ① `index_ms` 字段（FTS 重建）10.2s 是第二大项，可考虑增量重建而非全量 rebuild；② 分类器每文件 0.023ms，28 万文件 6.2s，`Classifier::new()` 的词典构建未跨批次复用（已在 `Arc` 里，但 `target.key()` 每次 format 分配）；③ 大文件（>10 分钟）仍整段解码，P1 的取舍未变；④ `docs/architecture.md` 5.1 的 9s 外推需要在文档里标注为「合成库条件」，避免下一任 agent 当成目标
- 下一步: P3 Tauri 外壳 + IPC 骨架（空窗口能调 `play` / `get_waveform`）

## 2026-11-16 12:30 · hermes · P0 拖拽导出通过 + P1 音频引擎原型落地

- 改: 新建 Rust workspace（根 `Cargo.toml` + `crates/reslip-engine` + `crates/reslip-cli`）；新增 `probes/tauri-drag-p0`（Tauri 2 拖拽验证窗口）；`docs/architecture.md` 补 P0/P1 实测结论；`docs/agent-log.md` 本条
- 状态: **P0 通过，P1 通过**。首个可运行的产品代码（engine crate + 命令行验收工具）

### P0 拖拽导出：通过

用户手操，从 Tauri 2 窗口把文件拖进 Cubase 16 / REAPER / FL Studio 均被接受并导入。验收标准（架构文档第 8 节）达成，外壳选型不用退回 Electron。

- 链路: `tauri-plugin-drag` 2.1.1 → `drag` 2.1.1 → `OleInitialize` + `SHCreateShellItemArrayFromIDLists` + `BHID_DataObject` + `DoDragDrop`，产出标准 `CF_HDROP`
- 自动侧验: 拖进一个普通 OLE 接收窗口，格式列表含 `FileDrop`/`FileNameW`/`Shell IDList Array`，路径与大小正确；单文件、多文件（4 个）、含中文与空格的文件名均通过
- **已知缺陷**: `plugin:drag|start_drag` 的 `onEvent` 回调从不触发。命令返回了，但 `Dropped`/`Cancel` 一次都收不到。产品里若要用它判断拖拽结果（比如「拖出后从库中移除」），需要另找办法，不能依赖这个通道
- 交互细节: 悬停拖拽项时光标样式不应变化（用户指出，留到产品阶段处理，不进探针）
- 遗留: 探针窗口的 UI 是临时验证页，不进产品

### P1 音频引擎原型：通过

命令行 `reslip-cli`，指定文件路径即播，打印实测延迟、连按切换无 glitch。

- **架构**: 双槽 + `arc-swap`，切换是一次原子写索引。旧 clip 由 `arc-swap` 等渲染线程的 guard 释放后才 drop，音频线程从不释放内存
- **渲染回调**: 只有 `copy_from_slice` 与 `fill(0)`。scratch 缓冲在循环外按 endpoint 上限预分配，回调内零分配、零加锁、零 IO、零格式化
- **`IAudioClient` 生命周期**: 在渲染线程内创建并持有，`set_get_eventhandle()` 的 `Handle` 与 `AudioClient` 同寿命
- **格式覆盖**: WAV / AIFF / FLAC / MP3 / OGG / M4A 六种全部可播，各自走完整管线验证（见下）
- **重采样**: `rubato` FFT（`new_custom` + `process_all`），整段一次转完。48k→44.1k、96k→44.1k 实测频率与幅度零误差（660.00Hz / 880.00Hz，峰值 0.350 不变）
- **声道映射**: 单声道→立体声复制，立体声→单声道取平均，宽布局兜底取现有声道

#### 实测延迟（本机 E2x2，`Playback 1/2 (2- E2x2)`，44100Hz 2ch f32）

口径按 AGENTS.md 区分两段：

| 口径 | 单次播放 | 连续切换（24–48 次） |
| --- | --- | --- |
| `publish → 回调写入` | 8.36–9.74ms | p50 2.5–5.9ms |
| 其中已排队（不可优化） | 12.00ms | 12.00ms |
| `publish → 出声` | **20.35–21.74ms** | p50 14.5–16.9ms，max 22.05ms |

- 三轮独立复测，数字稳定（切换 p50 三轮回 16.89 / 16.90 / 16.84ms）
- **口径修正**: 本机该设备的引擎周期是 **11.00ms**（endpoint 970 帧 = 22ms），不是调研记录的 10ms。设备与驱动组合不同周期不同，架构文档里的 17.5ms 是另一台设备的数字，不能直接当目标对照
- 切换的 p50 低于单次播放，因为处在稳态的队列更短；上界 22.05ms 与单次播放一致
- **下溢计数 0**：24 次与 48 次连切均无 missing report、无 endpoint 走空。这是「无 glitch」的机器判据，不依赖人的耳朵

#### 发现并修掉的两个真实缺陷

1. **MP3 编码器延迟 25.1ms**：`FormatOptions::enable_gapless` 默认 `false`，LAME 的 delay/padding 被忽略，解码结果开头多 1105 帧填充。开启后 MP3 与 ffmpeg 输出逐帧一致（44100 帧，lag=0，SNR 230dB）
2. **M4A 编码器 priming 1024 帧（23.2ms）**：symphonia 的 `isomp4` 解析了 `elst` 但没接到 `codec_params.delay`，gapless 对 M4A 无效。解码端按 AAC 惯例裁掉首 1024 帧（带「前 1024 帧是否真是 priming」的哨兵判断，阈值 -40dBFS 用 p90 而非最大值，因为 AAC priming 解出来是 -42dBFS 的低电平残渣而非纯零）。修后 M4A 起点与 ffmpeg 一致（0.02ms），中段 SNR 135dB

  这两个缺陷都直接吃延迟：23–25ms 与整个设备路径同量级，采样触发会明显慢半拍。

- 遗留: ① MP4/M4A 尾部仍多 956–1980 帧编码器填充（只影响末尾静音，不影响触发延迟，产品化时补尾部裁剪）；② OGG 尾部多 636 帧同类；③ 音频回调的 MMCSS 提升是 best-effort，未验证失败时的抖动代价；④ 切换时不做交叉淡化，硬切（架构文档 4.1 的可选项未实现）；⑤ 长文件仍整段解码进内存（P1 取舍，流式 decode-ahead 待 P2 之后）
- 验证: `cargo test --release` 5 个单测通过（声道映射、i16 转换、重采样时长）；六种格式各自渲染成 WAV 后用 ffmpeg/ffprobe 与源文件逐样本比对；真实设备三轮延迟复测
- 下一步: P2 索引器 + schema（对真实库跑一遍，与架构文档第 5 节数字对照）

## 2026-11-16 10:40 · hermes · 架构设计定稿 + WASAPI 第二轮实测

- 改: 新建 `docs/architecture.md`（完整架构决策）；重写 `docs/TODO-low-latency-audio.md` 的 Q1–Q6 答复与交付物；`docs/research-notes-playback.md` 新增第 6 节（WASAPI 重新实测）；`AGENTS.md` 更新 Stack 与 Rules
- 状态: **设计定稿，仍无产品代码**
- **核心纠正**: 第一轮的 8.33ms 口径有误。只计到「回调写入缓冲」，没算缓冲里已排队的 12ms。真实点击到出声是 **17.5ms**（min 12.0, max 21.9）
- **共享模式不可调**: 四台设备（E2x2 真实 USB / Realtek / FxSound / ToDesk）的 `GetSharedModeEnginePeriod` 全部返回 `min=default=max=10ms`；请求 882 帧被拒 `0x88890020`。换格式无效（扫了 f32/i16/i24in32 × 44.1k/48k/96k）
- **独占否决**: 持有独占后其它客户端（独占和共享都算）被拒 `0x8889000A` → 用户开着 DAW 会静音。这推翻了「ASIO 会抢设备」的原担心方向：真正抢设备的是 WASAPI 独占
- **三条优化路线全部实测无效**: ① 小周期 → 驱动拒绝；② 空闲不喂料 → 降到 5.6ms 但连续播放 250/252 次回调下溢；③ `Reset()` 清队列 → 清掉事件句柄，crate 0.27 重新注册报 `0x8000FFFF`
- **冷路径**: 7 种格式「打开+探测+解码首包」全部 < 0.4ms。解码从来不是瓶颈，Q1 的问题方向本身是错的
- **索引**: 2 万文件合成库实测，遍历 108ms + 探测 795ms（8 线程）+ 写入 128ms + 建索引 17ms ≈ 1.1s；外推 17 万约 9s（Slip Sound 现状 120s）
- **格式探测**: 7 种格式 header-only 都能拿 duration/channels/rate；唯一例外 M4A 读不到声道数（symphonia isomp4 返回 0ch），需解首包
- **拖拽导出风险解除**: CrabNebula 的 `drag` crate 2.1.1 有 Windows 实现（OLE + `DoDragDrop` + `CF_HDROP`），`tauri-plugin-drag` 2.1.1 封装成 Tauri 命令。本机验证 `drag` crate 编译通过
- 验证: 两个新探针项目（`%TEMP%\wasapi3-probe` 8 个 bin、`%TEMP%\index-probe` 3 个 bin）全部编译运行；延迟数字三轮独立复测；索引在 2 万文件库上跑通
- 遗留: ① 拖拽导出「拖进真实 DAW」未验（crate 只验证了编译）；② ASIO 真实出声延迟未用排队法复测（TOPPING 本轮未接）；③ 索引只有合成库数据，真实混合格式库待 P2 复测；④ `AGENTS.md` 的 "Where things live" 表缺 `architecture.md` 一行（写入被权限门禁拦下）
- 下一步: P0 拖拽导出实测 → P1 音频引擎原型（命令行，验证双槽切换无 glitch）

## 2026-11-16 09:50 · hermes · ASIO 打通并实测

- 改: `docs/research-notes-playback.md`（新增第 4 节 ASIO 实测 + 更新待补清单）、`docs/TODO-low-latency-audio.md`（Q3 标记部分已答、初步选型倾向）、`docs/prompt-for-research-session.md`（环境现状更新）
- 状态: 调研前置阻塞已清除，**仍无产品代码**
- 完成: ASIO 构建链路打通（`pip install libclang` 提供 DLL + ASIO SDK 预置到 `%TEMP%\asio_sdk`）；探针编译成功并实测
- **核心数据**: `TOPPING Pro` 支持 8–2048 帧 buffer；`Fixed(64)` 下回调 1.45ms、发布→出声中位 **0.68ms**、max 1.47ms；WASAPI 共享同机 8.33ms。ASIO 快约 12 倍且块大小无抖动
- **决定性发现**: ASIO 不是独占的。两进程并发开流，第二个实例 690 callbacks/s、0.69ms，性能无衰减。"DAW 开着变哑"的担忧在本机不成立
- 验证: 探针 `%TEMP%\asio-probe` 编译并跑三轮（64/128/256 帧 + 双实例并发）；WASAPI 与 ASIO 同一程序对比
- 遗留: 2ch 立体声配置未验（本轮只协商到 1ch）；真实 DAW 占用场景未复验；WASAPI 独占未测；ASIO SDK 许可证与分发方式未查（Q5）
- 下一步: 用 TODO 文档开调研对话。Q1 的 15ms 目标现已宽裕——设备侧只占 1.45ms 周期 + 0.68ms 延迟

## 2026-11-16 09:40 · hermes · 项目建立 + 调研前置实测

- 改: 新建 AGENTS.md、docs/TODO-low-latency-audio.md、docs/research-notes-playback.md、docs/agent-log.md
- 状态: **仅调研准备，无代码**。仓库已 git init，未提交远端
- 已完成: 参考机播放路径实测（symphonia 解码、WASAPI 常驻流 swap 延迟、回调几何、webview 对照），数据在 research-notes-playback.md
- 关键结论: 解码 1–6ms 不是瓶颈；WASAPI 共享模式周期固定 10.1ms 且无视小 buffer 请求；ASIO 是唯一能压到个位数毫秒的路
- 验证: 探针项目跑通并复测三轮（rust-audio-probe 的 main/geometry/decode_probe 三个 bin）
- 遗留: **ASIO 实测未采到**。asia-sys 构建需 LLVM/libclang，参考机没装；ASIO SDK 已手动解压到 %TEMP%\asio_sdk 绕过 rename 失败。这项是调研对话 Q3 的第一个阻塞
- 下一步: 用户拿 TODO 文档开新对话做调研；不开写实现

## 项目起点

从 `C:\Users\Naimio\projects\slip-sound`（Electron 版，fork 自 Stuyk/slip-sound）继承 UI/UX 与交互模型，重写后端。Slip Sound 侧的播放延迟优化与 Tauri 可行性分析已完成并推送（commit fd2f0cf）。
