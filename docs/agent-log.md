# Agent log

Shared handoff log. Newest entry on top. One entry per work session: what changed, what was verified, what is left.

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
