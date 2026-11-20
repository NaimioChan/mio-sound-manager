# Mio Sound Manager 架构设计

状态：**定稿并已实现发版（当前 0.1.2，见 `docs/agent-log.md`）**。本文回答「后端怎么做、栈怎么选」，不描述功能（功能与 UI/UX 沿用 Slip Sound，行为基准见 `C:\Users\Naimio\projects\slip-sound` 的 `AGENTS.md` 与 `src/renderer/src/components/WAVEFORM-PLAYBACK.md`）。延迟数字取自本文第 2、3、8.1 节的实测，索引数字来自第 5 节与 `docs/index.md`，均为本机实测；原始测量笔记已随调研阶段结束移除，需要复测时用 `probes/` 重跑。

## 1. 结论先行

| 决策 | 选择 | 一句话理由 |
| --- | --- | --- |
| 输出后端 | **WASAPI 共享模式（常驻流）为默认，ASIO 可选且已实现** | 共享模式不抢设备，实测 17.5ms；ASIO 在设置里由用户显式选择，实测 64 帧下 5.31ms |
| 不用 WASAPI 独占 | 否决 | 独占会让用户的 DAW 静音（实测其它客户端被拒 0x8889000A） |
| 音频输出实现 | **直接对接 `wasapi` crate，不用 cpal** | 需要 `IAudioClient3` 周期查询、`get_current_padding` 排队量、会话控制，cpal 都拿不到 |
| 解码 | **symphonia，全部格式特性打开** | 6 种目标格式全覆盖；冷启动首个包 < 1ms |
| 播放模型 | **常驻流 + 双槽交替 + 预解码邻行** | 把「切文件」从重建管线降级为换一个已解码的槽 |
| 索引存储 | **SQLite（rusqlite bundled）+ WAL + FTS5** | 2 万文件头探测 0.7s，插入 0.13s，索引构建 17ms |
| 索引并行度 | **探测阶段取逻辑核数**（探测是随机读延迟受限，不是 CPU 受限） | 合成库上 8 线程封顶；真实库无 DRAM SSD 上 8→16 线程还有 22% 收益。遍历与写入不并行 |
| 外壳 | **Tauri 2** | 体积与内存优势；webview 不参与音频，只做 UI |
| 前端 | **SolidJS + Siemens iX 5.3**（P7 换掉 Tailwind） | UI/UX 已定，交互模型不动；视觉与控件改为西门子设计系统的 web components，颜色统一走 `--theme-*` token，主题系统才能一处改全局变 |
| ffmpeg | **保留为外部进程，仅用于导出与特效渲染** | 播放路径完全不经过 ffmpeg；导出复用现有滤镜链 |

## 2. 为什么是 WASAPI 共享 + 可选 ASIO

### 2.1 三种后端的实测对比

同一台机器、同一台 USB 声卡（E2x2）、同一口径（publish → 首样本到达 DAC，含缓冲排队）：

| 后端 | 出声（中位） | 最差 | 抢设备 | 依赖 |
| --- | --- | --- | --- | --- |
| WASAPI 共享 | 17.5ms | 21.9ms | 否 | 无 |
| WASAPI 独占 | 4.5ms | 5.9ms | **是** | 无 |
| ASIO 64 帧 | 5.31ms（TOPPING Pro） | 5.31ms | 否 | ASIO SDK + libclang + 用户装驱动 |
| ASIO 128 帧 | — | — | 否 | 同上；周期 2.67ms |
| ASIO 256 帧 | 14.43ms（TOPPING Pro） | 14.43ms | 否 | 同上；周期 5.33ms |

ASIO 的数字是 P6 落地后在同一套 `publish → 出声` 口径下复测的（`LatencyReport::to_audible_ms`：回调写入时间 + 驱动报告的输出延迟）。第一轮那个 0.68ms 是「回调把数据交给驱动」，不含驱动/设备内部缓冲，不能与 WASAPI 的 17.5ms 并列，所以不用它。64 帧下 5.31ms 约为 WASAPI 共享模式的三分之一；256 帧下 14.43ms 反而接近 WASAPI——驱动缓冲本身就是延迟，帧数越大越快不了。

### 2.2 三个判断

**独占模式否决。** 实测持有独占后，第二个客户端（无论独占还是共享）直接被拒 `0x8889000A`。采样管理器是「编曲时随手找素材」的工具，用户必然开着 DAW；让 DAW 静音等于不可用。4.5ms 换不来这个代价。

**共享模式的 17.5ms 可以接受。** 这是本设计最重要的一次判断，依据是参照物而不是绝对值：

| 方案 | 点击到出声 |
| --- | --- |
| Slip Sound 现状（冷文件，未预载） | 60–100ms |
| Slip Sound 现状（连按方向键，池化预载命中） | 10–26ms |
| Mio Sound Manager（WASAPI 共享，任何文件） | 17.5ms |
| Mio Sound Manager（大文件冷启动，未预解码） | 17.5ms + 解码（3 分钟 96k/24bit 要 112ms） |

Mio Sound Manager 把最差情况从 60–100ms 压到 17.5ms，但**不能击败 Slip Sound 的池化预载命中路径**（10–26ms 与 17.5ms 是同一量级）。这是必须说清楚的地方：重写后端换来的不是「连按更快」，而是「冷文件不再慢」和「不用再维护元素池那套预载逻辑」。

预解码邻行仍然要做，但它的收益不是延迟——设备侧 12ms 排队躲不掉——而是**避免大文件冷启动时叠加 112ms 解码**。见 3.2。

**ASIO 保留为可选项，不作为默认。** 理由不是延迟不够（ASIO 确实更快，见上表），而是两条工程成本：构建需要 libclang（本机已解决，但每个开发者和 CI 都要处理）；没有 ASIO 驱动的用户占多数。SDK 的许可不构成障碍——它自己给出「专有许可或 GPLv3」二选一，本项目按 AGPL-3.0-or-later 发布，走的是 GPLv3 那一支，见 `THIRD-PARTY-NOTICES.md`。

P6 已按这个判断落地：`Backend` 枚举 + 两个渲染线程 + 共享的 `Mixer`，ASIO 在 `--features asio` 后编译进来，设置页里可选，默认仍是 WASAPI 共享模式。实现细节见 `docs/engine.md` 的「ASIO 后端」一节，其中 COM 套间（STA 而非 MTA）是唯一的硬约束。

### 2.3 关于「共享模式能不能再快」

实测过三条路，全部无效，记录在此以免重复试错：

1. **`IAudioClient3::InitializeSharedAudioStream` 申请更小周期**：本机四台设备（E2x2、Realtek、FxSound、ToDesk）的 `GetSharedModeEnginePeriod` 都返回 `min = default = max = 10ms`，请求 882 帧被拒 `0x88890020`。
2. **空闲不喂料排空队列**：能把 17.5ms 压到 5.6ms，但连续播放时 250/252 次回调下溢，听感断续。
3. **`Reset()` 丢弃排队音频**：`Reset()` 会清掉事件句柄，`wasapi` 0.27 无法重新注册（`0x8000FFFF`），路径不通。

结论：17.5ms 是这台机器上 WASAPI 共享模式的真实下限。

## 3. 延迟预算

### 3.1 冷文件（从未播放过）

以 3 秒 MP3（最慢的常见格式）为例：

| 阶段 | 耗时 | 说明 |
| --- | --- | --- |
| 决定播哪个文件 → 打开文件 | 0.09ms | `File::open` |
| 探测容器格式 | 0.09ms | symphonia probe |
| 解码首个包 | 0.02ms | 1 个 packet |
| 转 f32 交到音频线程 | ~0.1ms | 无重采样时是 memcpy |
| 写入环形缓冲 | ~0.05ms | |
| **解码小计** | **约 0.4ms** | 实测「open + 首包」< 0.4ms |
| 等音频回调取走 | 0–10ms | 撞上回调边缘 0ms，否则等一个周期 |
| 已排队音频播完 | 12.0ms | 恒定 |
| **设备侧小计** | **12–22ms** | |
| **合计** | **约 12.5–22ms（中位 17.5ms）** | |

关键含义：**解码不是瓶颈，连 0.5ms 都不到。** 之前担心的「冷文件要等解码」不成立；瓶颈 100% 在设备缓冲。这简化了整个设计：不需要为「首段解码」做特殊优化，直接整文件解码即可（见 3.3）。

### 3.2 预解码命中（连按方向键）

| 阶段 | 耗时 |
| --- | --- |
| 选中已解码槽 | < 0.05ms |
| 写入环形缓冲 | ~0.05ms |
| 设备侧（同上） | 12–22ms |
| **合计** | **12–22ms** |

注意：**即使 PCM 已经在内存里，也躲不掉 12ms 排队。** 这是共享模式的物理下限，预解码省的是解码时间，不是设备时间。所以对几秒钟的小素材，预解码在延迟上几乎无感（0.4ms vs 0.05ms）。

预解码真正的作用是大文件：3 分钟 96k/24bit 整文件解码 112ms，冷启动会变成 112 + 17.5 ≈ 130ms，用户能明显感到卡顿。预解码邻行把这一类从「明显卡顿」拉回 17.5ms。因此预解码池只对超过阈值的文件生效（见 3.4）。

### 3.3 解码策略：整文件解码，不做流式

依据实测：整文件解码 3 秒素材 1–6ms，30 秒 17ms，3 分钟 96k/24bit 112ms；而首个包解码都在 0.03ms 以内。采样管理器面对的绝大多数文件是几秒到几十秒。

策略：

- **小于阈值（默认 60 秒）**：整文件解码进内存槽。
- **大于阈值**：只解码前 5 秒进槽立即出声，其余在播放中后台补齐（decode-ahead）。首 5 秒覆盖了绝大多数试听场景（点一下听一耳朵）。
- **seek 到未解码位置**：实测七种格式都能在探测阶段拿到 n_frames 并 seek（见 5.2），所以直接 `seek(Accurate)` 到目标位置再解码即可，不需要从头解一遍。等待期间静音，不输出错误音频。

不做全流式的原因：流式的复杂度（环形缓冲下溢处理、解码线程与回调线程的同步）换不来延迟收益，因为瓶颈在设备侧。

### 3.4 内存预算

| 项 | 预算 |
| --- | --- |
| 当前播放槽（整文件 PCM，≤60 秒） | ≤ 60 秒 × 48k × 2ch × 4B ≈ 23MB |
| 预解码邻行（默认 ±2 行，仅 >阈值 的文件） | 4 × 5 秒首段 ≈ 4 × 2MB ≈ 8MB |
| 波形峰值缓存 | 1000 点 × 4B × 文件数，可忽略 |
| **上限** | **256MB**，超出按 LRU 淘汰 |

预解码池只对**超过阈值（60 秒）**的文件生效，且只解码首 5 秒（见 3.3），所以每个条目约 2MB 而不是 23MB。短文件本身解码只要 0.4ms，不需要预解码。

## 4. 线程模型

```
┌──────────────────────────────────────────────────────────────┐
│ UI 线程（Tauri main / webview）                               │
│  - 渲染列表、波形                                            │
│  - 用户点击/按键 → invoke('play', {path, startAt})           │
└──────────────────────────┬───────────────────────────────────┘
                           │ Tauri command（异步，不阻塞 UI）
┌──────────────────────────▼───────────────────────────────────┐
│ 播放控制线程（PlaybackController，单线程）                    │
│  - 持有播放状态机                                            │
│  - 决定「哪个槽生效」并原子切换 active_slot 索引              │
│  - 管理预解码请求队列（给解码池派活）                        │
│  - 从不在实时路径上做 IO 或分配                              │
└──────────────────────────┬───────────────────────────────────┘
                           │ Arc<AtomicUsize> + 无锁 ring
┌──────────────────────────▼───────────────────────────────────┐
│ 音频渲染线程（MMCSS "Pro Audio" 优先级）                      │
│  - 等 WASAPI 事件 → 取可用空间 → memcpy 填数据               │
│  - 绝对禁止：分配、锁、文件 IO、日志、格式化字符串            │
│  - 下溢时填静音并计数（上报给控制器做统计）                  │
│  - IAudioClient 是 !Send，必须在本线程创建，不能从外部传入    │
└──────────────────────────┬───────────────────────────────────┘
                           │ WASAPI event
┌──────────────────────────▼───────────────────────────────────┐
│ WASAPI 共享流（常驻，进程启动时建立，直到退出）              │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ 解码线程池（rayon，4 线程）                                   │
│  - 冷文件解码、预解码邻行、长文件 decode-ahead                │
│  - 每个解码任务产出 Arc<DecodedAudio>（不可变，可共享）       │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ 索引线程（独立，与播放完全隔离）                              │
│  - 目录遍历 → 元数据探测（并行 rayon，线程数 = 逻辑核数）→ SQLite 批量插入     │
│  - 不持有任何播放状态，播放也不等它                          │
└──────────────────────────────────────────────────────────────┘
```

### 4.1 双槽设计

不用「一个缓冲 + 换源」，用**两个独立槽交替**：

```
slots: [Slot; 2]
active: AtomicUsize   // 0 或 1，音频线程只读这个
```

切换文件时：

1. 控制线程把新文件的 `Arc<DecodedAudio>` 放进 `slots[1 - active]`。
2. 控制线程把 `active` 从 0 翻到 1（一次原子写）。
3. 音频线程下一个回调读 `active`，从新槽取数据。

关键约束：**旧槽在新槽生效前不能被释放。** 用 `Arc` 的引用计数天然保证——音频线程在回调开始时 `Arc::clone` 当前槽，回调结束才 drop。控制线程切换后旧槽还活着，直到音频线程不再引用它。

这个设计让「切文件」变成一次原子写，没有锁、没有等待、没有 glitch 风险。交叉淡化（如果要做）在音频线程内对两个槽同时读，按 1–5ms 淡入淡出。

### 4.2 为什么不用 cpal

cpal 是跨平台抽象，它把「周期、缓冲、排队量」都藏在背后。本设计需要三件 cpal 给不了的东西：

- `IAudioClient3::GetSharedModeEnginePeriod`：判断设备到底能给多少周期（用于诊断页和 ASIO 对比）。
- `get_current_padding()`：测真实延迟、检测下溢。
- 会话控制（音量混音器里的独立条目、`set_display_name`）。

代价是失去 macOS/Linux 支持。这个项目**只做 Windows**（AGENTS.md 已定），代价为零。

### 4.3 采样率转换

设备 mix format 与文件采样率不同时（常见：设备 48k，素材 44.1k），必须重采样。选择：

- **优先用 WASAPI 的 `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`**（共享模式专用）：由引擎做转换，零代码。实测开关它不影响 12ms 排队。
- 需要自己控制质量时（导出预览、变速不变调），用 `rubato` 做重采样。

## 5. 索引子系统

### 5.1 实测数据（2 万文件合成库，NVMe）

| 阶段 | 1 线程 | 2 | 4 | 8 | 16 |
| --- | --- | --- | --- | --- | --- |
| 目录遍历 | — | — | — | — | 108ms（185k files/s） |
| 元数据探测 | 2530ms | 2005ms | 1281ms | **795ms** | 695ms |

| SQLite 写入 | 耗时 | 吞吐 |
| --- | --- | --- |
| 单事务，默认 journal | 136ms | 147k rows/s |
| 单事务，WAL | 175ms | 115k rows/s |
| 5k 行/事务，WAL，无索引 | 128ms | 156k rows/s |
| 后建索引（2 万行） | 17ms | — |

外推到 17 万文件（Slip Sound 的参考库规模）：

| 阶段 | 耗时 |
| --- | --- |
| 遍历 | ~1.0s |
| 元数据探测（并行） | ~6.8s |
| SQLite 写入 | ~1.2s |
| 索引构建 | ~0.15s |
| **合计** | **约 9s** |

对比 Slip Sound 现状约 120s。**提升约 13 倍**，主要来自并行探测与移除逐文件 `statSync` + Node 层开销。

> **这组数字是合成库条件，不是真实库的目标。** 2 万文件由脚本预生成、放在 NVMe 上、文件头结构统一。P2 在真实库（28 万文件，混合来源，无 DRAM 的入门 SSD）上实测全量 63.9s、增量 5.4s，与 9s 外推差 7 倍。差异来源已定位：探测阶段的 `stat + open + 读 64KB` 占 80%，解析只占 20%，瓶颈是随机读延迟而不是解析代码——同一份代码在热缓存小样本上跑 10,790 文件/s，冷缓存全库 4,247 文件/s。详见 `docs/agent-log.md` 的 P2 条目。
>
> 结论：**稳态增量索引是 5.4s 量级，这才是用户会遇到的数字**；全量重扫只在首次和强制重建时发生。不要再拿 9s 当验收线。

### 5.2 只读文件头，不解码

实测 7 种格式的 header-only 探测（symphonia probe，不建解码器）：

| 格式 | 探测耗时 | duration | channels | sample_rate | 可 seek |
| --- | --- | --- | --- | --- | --- |
| WAV 16/24/32f | 0.09–0.45ms | ✅ | ✅ | ✅ | ✅ |
| AIFF | 0.17–0.31ms | ✅ | ✅ | ✅ | ✅ |
| FLAC | 0.12–0.21ms | ✅ | ✅ | ✅ | ✅ |
| MP3 | 0.13–0.33ms | ✅ | ✅ | ✅ | ✅ |
| OGG Vorbis | 0.25–0.39ms | ✅ | ✅ | ✅ | ✅ |
| AAC（裸流） | 0.17–0.29ms | ✅ | ✅ | ✅ | ✅ |
| M4A/AAC | 0.19–0.35ms | ✅ | ❌ **0ch** | ✅ | ✅ |

唯一缺陷：**M4A 从头部读不到声道数**（返回 0），必须解码第一个包。这是 symphonia 的 `isomp4` 行为，不是 bug。处理：M4A 走「探测 + 解码首包」的慢路径，成本约 0.3ms，仍可忽略。

> **P2 实测补充：WAV 还有一个 symphonia 不认的头部变体。** `symphonia-format-riff` 的 `read_pcm_fmt` 只接受 `fmt ` chunk 长度 16 / 18 / 40，其它长度报 `malformed fmt_pcm chunk`。20 字节是合法的 `WAVEFORMATEX`（`cbSize = 2`），真实库 28 万文件里有 284 个是这种，全部可正常播放。`crates/mio-index/src/wav_header.rs` 做 RIFF 头兜底解析，只在 symphonia 拒绝时启用。

**七种格式都能在探测阶段拿到 n_frames 并成功 seek 到 1 秒处**，所以「seek 到未解码位置」不需要先解一遍——直接 seek 再解即可。这简化了大文件的 seek 实现。

### 5.3 增量索引

判断文件是否变化用 `(mtime, size)` 组合。SQLite schema 里存 `mtime_ns` 与 `size`，遍历时对比，未变化跳过探测。实测目录遍历本身只占 1s，所以增量索引的开销几乎全在「有多少文件变了」。

### 5.4 Schema

沿用 Slip Sound 的表结构（`sounds` + `sounds_fts` + 触发器），新增两列：

```sql
ALTER TABLE sounds ADD COLUMN mtime_ns INTEGER;   -- 增量索引判断
ALTER TABLE sounds ADD COLUMN indexed_at INTEGER; -- 诊断用
```

FTS5 配置不变（`unicode61 remove_diacritics 2`，索引 `filename`/`category`/`subcategory`/`folder`）。触发器不变。这样 `src/main/db.ts` 的查询逻辑可以逐条对照翻译，降低搬迁风险。

索引期间 UI 要能查询，所以必须 WAL。实测 WAL 单事务比默认 journal 慢 39ms（175 vs 136），但换来并发读，值。

## 6. 技术栈

### 6.1 已定

| 层 | 选择 | 版本基线 |
| --- | --- | --- |
| 语言 | Rust | 2024 edition |
| 外壳 | Tauri 2 | 2.x |
| 前端 | SolidJS + `@siemens/ix` 5.3 web components | P7 定；P7 前短暂用过 Tailwind 4 + lucide-solid，已换 |
| 主题 | 自研推导：八色调色板 → 两百多个 `--theme-*` token | 见 `ui/src/theme.ts` 与 `docs/app.md` |
| 多语言 | 中英日三语，英文键集基准 | `ui/src/i18n.ts` + `locales/{en,zh,ja}.ts` |
| 解码 | symphonia（`all-formats` + `all-codecs`） | 0.5 |
| 音频输出 | `wasapi` crate | 0.27 |
| 重采样 | `rubato` | 最新 |
| 数据库 | `rusqlite`（bundled SQLite） | 0.37 |
| 并行 | `rayon` | 1.11 |
| 目录遍历 | `walkdir` | 2.5 |
| 导出/特效 | ffmpeg 外部进程（现有滤镜链） | 系统 ffmpeg 或 bundled |
| 测试 | `cargo test`（Rust）+ `tools/check-*.mjs`（CDP 驱动真实窗口） | — |

### 6.2 选型理由

**Tauri 2 而不是继续 Electron。** 理由不是延迟（webview 不参与音频，见第 4 节线程模型），而是：安装包从 ~150MB 降到 ~15MB；空闲内存从 ~200MB 降到 ~80MB；Rust 后端与音频引擎同进程，IPC 是函数调用而非序列化。UI 是纯展示层，换成 webview 没有行为损失。

**`wasapi` 而不是 `cpal`。** 见 4.2。

**`rusqlite` bundled 而不是 `sqlx`。** 索引是同步的、批量的、单进程的，不需要异步连接池。bundled 省掉用户装 SQLite 的步骤。`sqlx` 的编译期查询检查在这里没有收益（schema 简单）。

**ffmpeg 保留外部进程。** 导出、特效渲染、LUFS 测量这些是离线任务，用 ffmpeg 的滤镜链比在 Rust 里重写划算得多（`specs/processing.md` 已经定义了一整套 ffmpeg 滤镜语义）。播放路径完全不碰 ffmpeg，所以它的启动开销不影响延迟。

### 6.3 待验证的风险

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| **Tauri 2 的拖拽导出在 Windows 上可用性** | ~~高~~ **已解除（2026-11-16）** | `tauri-plugin-drag` 2.1.1 → `drag` 2.1.1 → `OleInitialize` + `SHCreateShellItemArrayFromIDLists` + `BHID_DataObject` + `DoDragDrop`，产出标准 `CF_HDROP`。实测拖进 Cubase 16 / REAPER / FL Studio 均被接受，单文件、多文件、含中文与空格的文件名都通过。验证程序在 `probes/tauri-drag-p0`。**遗留**：`start_drag` 的 `onEvent` 回调从不触发，产品里不能用它判断拖拽结果 |
| ASIO 可选后端的构建复杂度 | 中 | 用 feature flag 隔离，默认不编译 |
| 独占模式在 ASIO 下是否也抢设备 | 中 | 需开着真实 DAW 复测（上轮只测了 cpal 双实例） |
| 变速不变调（stretch）的实现 | 中 | ffmpeg 的 `atempo` 只在导出用；实时预览需要 Rust 侧时间伸缩，可能要用 `signalsmith-stretch` 或降级为「预览不处理变速」 |
| 实时 LUFS 测量 | 低 | 现有实现是 ffmpeg `ebur128` 离线测量，沿用 |

## 7. 前端与后端的边界

### 7.1 原则

**渲染层不碰音频数据。** 波形峰值由后端算好通过 IPC 传（1000 个 f32 = 4KB，可忽略）；PCM 永远不进 webview。

### 7.2 IPC 接口

按 Slip Sound 的 preload 接口逐条映射，用 Tauri command 替代 `ipcMain.handle`。P3 时只有播放的四条原型命令；现在共 34 条，完整清单与每个命令的语义见 `docs/app.md` 的「IPC 表面」一节（那里是唯一需要维护的表，本节不复制）。按子系统分组：

| 组 | 命令 |
| --- | --- |
| 状态与设备 | `get_status`、`list_audio_devices`、`asio_compiled`、`set_audio_device` |
| 播放 | `play`、`stop`、`pause`、`resume`、`get_playback_position` |
| 输出与波形 | `set_gain`、`get_gain`、`set_loop`、`get_loop`、`get_waveform`、`export_region` |
| 库与索引 | `add_library`、`list_libraries`、`remove_library`、`set_active_library`、`reindex_library`、`get_library` |
| 浏览与搜索 | `search`、`count_matching`、`list_matching_paths`、`list_folders`、`get_sidebar` |
| 编辑与撤销 | `set_favorite`、`set_category`、`clear_category`、`undo`、`redo`、`get_history` |
| 系统 | `pick_library_folder`、`reveal_in_folder` |

这张表只说明分组，不重复签名：线上字段一律 camelCase，前端类型手写在 `ui/src/ipc.ts` 与 Rust 侧一一对应，改任何一条要同时改三处（`commands.rs` 的线上类型、`lib.rs` 的 `invoke_handler`、`ipc.ts`）。

### 7.3 播放状态回传

webview 需要知道「播到哪了」来画播放头。不用事件推送（每次回调都发事件会淹没 IPC），用**轮询**：

- 前端 `requestAnimationFrame` 里调 `get_playback_position`（几个原子读，成本 < 1µs）。
- 后端维护 `pos_frames` 与 `active_frames`，音频线程每回调写前者；后者随 publish 一起写，所以时长不需要碰槽位。

返回的是 `PlaybackPosition` 结构而不只是一个秒数：`positionS`、`durationS`、`playing`、`paused`、`finished`、`looping`、`loopRegionS`。UI 靠 `playing` 与 `paused` 决定空格键是暂停还是继续、靠 `finished` 决定按钮回不回 Play，都是引擎的事实而不是渲染层自己记的状态。

这比 Slip Sound 现在的 `timeupdate` 事件更准（`timeupdate` 只有 ~4Hz）。

**位置口径是「回调写到哪」，不是「听众听到哪」。** 共享模式下两者差一次排队量（约 12ms）。画播放头用前者是正确的：它随音频推进得平滑，而补偿那 12ms 需要知道设备实际排队深度，轮询路径拿不到也不该拿。

## 8. 实施顺序

按「先证伪高风险项」排序，每一步都有可验收的产物：

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| **P0** ✅ | 拖拽导出可行性验证 | 能从一个最小 Tauri 2 窗口把文件拖进真实 DAW。**2026-11-16 通过**：Cubase 16 / REAPER / FL Studio 均接受并导入 |
| **P1** ✅ | 音频引擎原型（不含 UI） | 命令行程序，指定文件路径即播，打印实测延迟；连按模拟切换无 glitch。**2026-11-16 通过**：`crates/mio-engine` + `crates/mio-cli`，六格式可播，切换下溢 0 |
| **P2** ✅ | 索引器 + schema | 对真实库跑一遍，打印各阶段耗时，与第 5 节数字对照。**2026-11-16 通过**：`crates/mio-index`，28 万真实文件全量 63.9s、增量 5.4s。当时的分类词典移植自 Slip Sound，与旧库 271,350 行零差异；**2026-11-17 起词典与打分模型改为本项目自有**（GPL 汇编不能搬运，见 `docs/index.md` 的 classify 一节），未分类比例从 43.1% 降到 4.3%，全量重索引 68s。与 5.1 的 9s 外推不符，原因见 5.1 末尾 |
| **P3** ✅ | Tauri 外壳 + IPC 骨架 | 空窗口能调 `play`/`get_waveform`。**2026-11-16 通过**：`crates/mio-app` + `ui/`，22 个 Rust 测试（其中 15 个走真实设备与真实索引，不开窗口）；真实 28 万库打开到首屏 69ms、搜索 65ms、点击到波形 30–43ms。窗口内交互用 CDP 驱动断言（播放头推进 / seek / 停止），细节见 `docs/app.md` |
| **P4** ✅ | 前端搬迁（列表、波形、搜索） | 能浏览、搜索、点击播放。**2026-11-16 通过**：列表虚拟化 + 键盘/多选/右键/拖拽导出、分类侧栏 facet 计数、原生目录选择、收藏与手动分类（批量事务，扛得住全量重索引）、空格暂停。`cargo test --release --workspace` 83 个通过；CDP 驱动真实 28 万库 40 项交互断言全绿，DOM 几何断言无重叠无裁切；点击到波形 30–36ms，Ctrl+A 选中 271,350 条路径。细节见 `docs/app.md` |
| **P5** ✅ | 工作区（多库、持久化、undo、阻塞索引、设置） | 能当日常工具用。**2026-11-16 通过**：同时打开多个库并在侧栏分组切换、启动恢复上次的库、无限翻页、收藏与分类的 undo/redo、索引期间锁窗并显示读条、设置页、可拖拽宽度的侧栏、搜索框移到列表列顶部。`cargo test --release --workspace` 97 个通过；CDP 三份脚本（工作区 26 项、索引遮罩 7 项、P4 回归 40 项）全绿 |
| **P6** ✅ | ASIO 后端 + 设置里的音频设备选择 | 用户试用后追加。**2026-11-16 通过**：`--features asio` 编译进 ASIO 后端，设置页有 Audio 节（WASAPI 端点与 ASIO 驱动一个下拉，ASIO 选中时多一个缓冲大小），选择持久化到 `audio.json` 并在启动时恢复。本机 TOPPING Pro 实测：128 帧周期 2.67ms、`publish → 出声` 5.13ms（64 帧）/ 14.4ms（256 帧），零下溢。`cargo test --release --workspace` 101 个通过；CDP 四份脚本全绿 |
| **P7** ✅ | 改名 Mio Sound Manager + 前端按西门子 iX 重写 + 配色主题系统 + 输出音量 | **2026-11-17 通过**：crate/二进制/identifier/localStorage 前缀全部改名；前端用 `@siemens/ix` 5.3 web components 重写，布局与交互不变；六套主题由八色调色板推导两百多个 token；dock 加静音按钮 + 滑杆 + 百分比，增益存 `Shared.gain_bits`，渲染回调按设备采样格式原地缩放、不重新解码。`cargo test --release` 62 通过；CDP 换四份脚本（interactions 24 项 / layout / themes 6 套含 WCAG / volume）全绿 |
| **P8** ✅ | 中英日三语 + 图标本地化 | **2026-11-17 通过**：`i18n.ts` + `locales/{en,zh,ja}.ts`，英文是键集基准、另两语 `Record<MessageKey, string>` 漏键即编译错误；后端 undo/redo 由英文 label 改为 `EditKind` 枚举；11 个图标 SVG 复制进仓库、`?raw` 拼 data URL（`ix-icon` 按名字会 fetch 不存在的 `svg/<name>.svg`，`encodeURIComponent` 会被 iX 的 `decodeURI` 解坏）。`cargo test --release` 105 通过；`check-i18n` 151×3 键齐平 |
| **P9** ✅ | BPM 排序、tag 多选、文件夹自绘菜单、去掉副标题 | **2026-11-17 通过**：BPM 只从文件名取（`crates/mio-index/src/bpm.rs`），空值正反方向都排最后；分类与子分类 Ctrl+点击多选，两侧筛选语义分别是 OR 与 AND；文件夹行复用 `ContextMenu`；设置页删掉解释性小字与主题卡片描述。`check-tags` 45 项 |
| **P10** ✅ | 波形框选、循环播放、选区拖出、静音记忆、拖拽修复 | **2026-11-18 通过**，发版 v0.1.1。行 `draggable` 写成裸属性会渲染成 `draggable=""`，枚举属性空串不合法 → 浏览器回落 `auto` → DOM 拖拽永不发起，整条拖进 DAW 的功能静默失效（判据：修复前 `row.draggable === false`）。循环是引擎状态：三个原子 + `Mixer::fill` 分段回绕，改选区不重新发布。选区按源格式写 WAV 进缓存再交给 `startDrag`，因为没有 DAW 接受帧范围。静音记 `mio:unmute-gain`，取消静音回到用户调的值。`cargo test --release --workspace` 全通过；`check-region` 16 项 + `check-volume` 12 项，其余四份脚本全绿 |

**路线图到 P10 为止。** 「Slicer / Effects / Loop / Export」小工具仍不在路线图内：这一版的目标是一个好用的采样浏览器，不是 Slip Sound 的功能全集。P10 加的循环是**试听用的循环播放**（把一段素材反复听），不是 Loop 工具（切片后重排），两者不要混淆。ASIO 曾按「不做」的逻辑被移出，用户试用后重新要求，因此以 feature 形式实现（见第 2 节）——用户的实际反馈优先于本表的边界。

P0 必须先做：如果拖拽导出在 Tauri 2 + Windows 上无解，整个外壳选型要重新考虑（退回 Electron 的代价是体积和内存，但能保住交互）。

### 8.1 P1 实测结果（本机，E2x2 `Playback 1/2`，44100Hz 2ch f32）

引擎周期 **11.00ms**（endpoint 970 帧 = 22ms），不是本文其他章节记录的 10ms——周期由设备与驱动决定，换设备就换数字。`publish → 出声` 含共享模式恒定的 12ms 排队。

| 口径 | 单次播放 | 连续切换 p50 | 连续切换 max |
| --- | --- | --- | --- |
| `publish → 回调写入` | 8.36–9.74ms | 2.5–5.9ms | 9.7ms |
| 其中已排队 | 12.00ms | 12.00ms | 12.00ms |
| `publish → 出声` | 20.35–21.74ms | 14.5–16.9ms | 22.05ms |

24 次与 48 次连切的下溢计数均为 0。切换 p50 低于单次播放是因为稳态下队列更短。

**两个会直接吃掉延迟的解码缺陷**（已修，细节见 `docs/engine.md`）：MP3 的 LAME encoder delay 25.1ms（`enable_gapless` 默认关闭）、M4A 的 AAC priming 1024 帧 23.2ms（symphonia 的 isomp4 解析了 `elst` 但没暴露）。二者都与整个设备路径同量级。

## 9. 明确否决的路线

| 路线 | 否决依据 |
| --- | --- |
| Web Audio / `<audio>` 播放 | 锁在 Chromium 音频线程，无法控制周期，无法用 ASIO；实测 32–40ms 换源开销 |
| WASAPI 独占 | 实测让其它客户端被拒 `0x8889000A`，DAW 会静音 |
| ASIO 作为默认 | 构建需 libclang；用户多数没驱动 |
| 空闲不喂料换低延迟 | 连续播放 250/252 次回调下溢，听感断续 |
| `Reset()` 丢弃排队音频 | `wasapi` 0.27 路径不通（`0x8000FFFF`），且 `Reset` 清事件句柄 |
| 预转码到统一格式 | 索引阶段变转码农场；实测解码已经够快，无需此路 |
| 流式解码 | 瓶颈在设备侧 12ms 排队，流式省不下延迟却引入下溢复杂度 |
| 全文件预载到 webview | PCM 走 IPC 每 MB 几毫秒，且内存翻倍 |
