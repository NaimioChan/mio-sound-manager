# Mio Sound Manager 音频引擎

`crates/mio-engine` 的模块细节。架构依据见 `docs/architecture.md` 第 4、8 节；本文件只说代码怎么组织、为什么这么切。实测数字见 `docs/agent-log.md` 与 `docs/architecture.md` 8.1。

## 模块

```
src/
  lib.rs      // 对外导出，模块职责一句话
  error.rs    // EngineError，解码失败带路径（UI 要显示「这个文件播不了」）
  decode.rs   // 文件 → 设备格式 PCM，可在任意线程跑
  output.rs   // 双槽调度 + WASAPI 常驻流
  asio.rs     // ASIO 后端（仅 --features asio）
  waveform.rs // 文件 → ~1000 个峰值（UI 画波形用，PCM 不出进程）
```

decode 与 output 的契约是 `Arc<DecodedAudio>`：不可变、已转成设备格式、可随意共享。解码侧怎么慢都行，渲染侧只认这一份字节。waveform 复用 decode 的解码结果，但从原始采样率的交错 f32 取峰值，不经过设备格式转换。

## decode

`Decoder::decode(path)` 做四件事，全部在调用线程上完成：

1. symphonia 探测 + 解码到交错 f32（用文件自己的采样率与声道数）
2. 采样率不同则 `rubato` FFT 整段重采样到设备采样率
3. 声道映射到设备声道数
4. 转成设备样本格式（f32 / i16 / i32）的字节流

**为什么整段解码。** 采样管理器的素材是秒级长度，整段常驻是最简单的「不会因为磁盘慢而欠载」的方案。长文件的流式 decode-ahead 是 P2 之后的事。

**为什么在解码侧做重采样而不是交给 WASAPI 的 `AUTOCONVERTPCM`。** 两者都开：`AUTOCONVERTPCM` 是兜底，自己转是为了让 `DecodedAudio` 的字节与 `OutputFormat` 严格一致，否则 `frame_bytes` 对不上时渲染线程只能丢帧。

**两个必须开 gapless 的理由**（实测，数字见 `docs/agent-log.md`）：

- `FormatOptions::enable_gapless` 默认 `false`。关闭时 MP3 不裁 LAME 的 encoder delay，开头多 1105 帧（25.1ms）
- 即使开了，M4A/AAC 也不裁：symphonia 的 `isomp4` 解析了 `elst` 但没接到 `codec_params.delay`。`decode` 里按 AAC 惯例裁掉首 1024 帧，前置一个哨兵判断（前 1024 帧的 p90 幅度低于 -40dBFS 才裁），避免把真的有内容的开头切掉

哨兵阈值不能设成「接近零」：AAC priming 解出来是低电平残渣（实测峰值 -42dBFS），不是纯零。

## output

```
Engine::start()  →  渲染线程（创建 IAudioClient、起流、进循环）
Engine::publish() →  存槽、打时间戳、翻 active
```

**两个后端，一套槽。** `output.rs` 里的 `Shared` + `Mixer` 与后端无关：`Mixer::fill()` 从活跃槽拷出交错的设备格式字节，把剩余部分补静音，返回真实音频的字节数。WASAPI 直接把这份 `scratch` 写进 endpoint；ASIO 把它拆成每声道一条 plane 写进驱动的双缓冲（`asio.rs`）。

后端由 `EngineConfig::backend` 选，默认 `Backend::Wasapi`。ASIO 只在 `--features asio` 时编译进来（见下）。

**槽与切换。** `slots: [ArcSwapOption<DecodedAudio>; 2]` + `active: AtomicUsize`。

`publish` 的顺序是：写入非活跃槽 → 重置位置与 finished → 打 `pub_seq` 与 `pub_ns` → 翻 `active`。渲染线程先读 `active` 再读 `pub_seq`，看到新序号就说明槽里的东西已经装好了。

旧 clip 由 `arc-swap` 负责回收：`store()` 内部会等所有 guard 释放才 drop 旧值，所以音频线程永远不会释放自己正在读的内存。**不要**自己写 retire 列表，那是重复实现而且还容易错。

**渲染回调的硬约束。** `scratch` 缓冲在循环外按 endpoint 上限分配一次；回调内只做 `copy_from_slice` 与 `fill(0)`，没有分配、没有锁、没有 IO、没有 `format!`。测量数据通过 `ArrayQueue` 送出，push 失败就丢弃——诊断数据不值得阻塞音频线程。ASIO 的每声道拆分同样是预分配指针上的定长循环。

**`IAudioClient` 的生命周期。** 它在渲染线程内创建、使用、销毁。`set_get_eventhandle()` 返回的 `Handle` 与 `AudioClient` 同寿命；提前 drop 会关掉事件，`Start()` 报 `AUDCLNT_E_EVENTHANDLE_NOT_SET`（`0x88890014`）。

**设备格式限制。** WASAPI 只支持 f32/32bit 与 i16/16bit，共享模式设备实际只发这两种；其它情况报 `Unsupported` 而不是猜字节布局。ASIO 额外支持 i32（`ASIOSTInt32LSB` 及其 16/18/20/24 位对齐变体），并处理大端驱动的字节序。

**周期口径随后端变。** `engine_period_ms()` 在 WASAPI 下是 `endpoint/2`（endpoint 缓冲等于两个引擎周期），在 ASIO 下就是 buffer 本身（一次回调填一个驱动缓冲）。所以同一个字段在 ASIO 128 帧下是 2.67ms，在 WASAPI 共享模式是 11ms，这不是算错。

## ASIO 后端（`--features asio`）

```
src/asio.rs
  render_thread()  // STA 初始化 → 加载驱动 → 建缓冲 → start → 睡到停止
  open_stream()    // 协商采样率/样本类型/声道，注册回调
  list_devices()   // 枚举 + 逐个探测（通道、首选缓冲、采样率）
```

**为什么是 feature。** ASIO SDK 是 Steinberg 专有许可，不能随 GPL 二进制分发；`asio-sys` 用 bindgen 构建它，需要 `libclang.dll`。crate 的默认 feature 因此完全不碰它（裸 `cargo build` / `cargo test` 不需要 SDK），产品构建走 `./tools/build-app.sh`，它默认就带 ASIO；`--no-asio` 才构建纯 WASAPI 的开发版。

**构建前提。** SDK 解压到 `%LOCALAPPDATA%\mio\asio_sdk`（`CPAL_ASIO_DIR` 可覆盖；放在 `LOCALAPPDATA` 而不是 `%TEMP%`，因为 Windows 会清临时目录，构建前提不能是被随时回收的东西），`LIBCLANG_PATH` 指向含 `libclang.dll` 的目录（pip 的 `libclang` wheel 就带一个）。`./tools/build-app.sh` 默认就带 ASIO；`--no-asio` 才构建纯 WASAPI 的开发版。

**COM 单元是 STA，不是 MTA。** 这是本项目唯一一个只有实测才能发现的坑。ASIO 驱动对象由 `CoCreateInstance` 创建（SDK 的 `asiolist.cpp` 里的 `asioOpenDriver`），而 TOE 驱动只从单线程套间交出对象。本机实测：

| 线程 | COM 初始化 | 加载结果 |
| --- | --- | --- |
| 主线程 | 无（默认 STA） | 成功 |
| 工作线程 | 无 | `loadDriver` 返回 false |
| 工作线程 | MTA（`initialize_mta`） | `loadDriver` 返回 false |
| 工作线程 | STA（`initialize_sta`） | 成功 |

所以 ASIO 的渲染线程是 STA，WASAPI 的渲染线程是 MTA，两者不能共用一个线程。症状是「探针能加载、应用里加载失败」，看起来像权限或驱动问题，其实是套间。

**回调在驱动的线程上跑。** `ASIOStart()` 立刻返回，之后驱动自己调 `bufferSwitch`；渲染线程只是睡到 `running` 变 false。所以 MMCSS 提升要对启动线程做（驱动沿用它的优先级），而不是对某个循环做。

**回调注册是全局的。** `asio-sys` 用进程级注册表，退出顺序必须是 `stop()` → `remove_callback()`，否则下一次启动会拿到指向已释放缓冲的回调。

**只开两个声道。** 驱动有 6 个输出时只开前两个：采样管理器不播环绕，多开只会多付拆 plane 的拷贝。

**枚举会真加载驱动。** `list_devices()` 逐个 `load_driver` 读通道/缓冲/采样率再 drop。拔了线的驱动会在这一步失败，此时条目仍然列出但标 `available: false`——「你的接口没插」比「列表里什么都没有」有用。


## 给 UI 的入口（P3 加播放，P4 加暂停）

`publish` / `publish_from` / `stop` / `pause` / `resume` 都取 `&self`：它们只写原子量与 `arc-swap`，不需要独占引用。IPC 的命令线程因此可以直接发布，不必再搭一个控制线程把调用排队。

- `publish_from(clip, offset_frames)` 是 IPC `start_at` 的落点。seek 不重新解码，只是在不可变缓冲上换一个起始位置，代价与普通播放相同的一次原子写。
- `stop()` 清空两个槽并把 `finished` 置位。它不碰流，所以下一次 `publish` 仍是一次原子写。共享模式排不掉已进 endpoint 的音频，停会有一小段尾巴，这是设备行为不是实现缺陷。
- `monitor()` 返回 `PlaybackMonitor`：`snapshot()` 与 `drain_reports()` 都是原子读 / 无锁队列读。播放头每秒被问 60 次，不能取 `arc-swap` guard（会跟音频线程自身的 guard 抢），所以开播时把 clip 总帧数一并存进 `active_frames`，`snapshot` 不碰槽位。

### pause / resume：冻结而不是清空

`stop` 与 `pause` 解决的是两件不同的事，混用会让空格键的行为错掉：

| 命令 | 槽位 | `pos_frames` | `finished` | 听感 |
| --- | --- | --- | --- | --- |
| `stop` | 两个都清 | 归零 | 置位 | 一小段尾巴后静音 |
| `pause` | 保留 | 原地不动 | 不动 | 一个引擎周期内静音 |
| `resume` | 保留 | 原地不动 | 不动 | 从停下的那一帧继续 |

`pause` 只翻一个 `AtomicBool`。渲染回调看到它就写静音、**不更新** `pos_frames`，所以播放头冻结在听众听到的那一帧，而不是回调已经写到的地方——这两者在共享模式下差 12ms 的排队量。`resume` 把标志翻回去，clip 还在槽里，因此不重新解码、不重新 seek、不重新发布。

三个配套决定：

- **`publish` 会清掉 `paused`。** 留着它会让新发布的音频静音播放，那是唯一看起来像引擎坏掉的状态。
- **暂停期间不计下溢。** 下溢的判据是「有 clip 要播但 endpoint 空了」，暂停时写静音是正确行为，不该被记成 glitch。
- **`resume` 在没有 clip 时是 no-op。** 空 dock 上误按空格不该伪造出状态。

`PlaybackSnapshot` 因此多了 `paused` 字段，`playing` 与它是互斥的：`playing = frames > 0 && !finished && !paused`。UI 靠这两个字段决定空格键是「暂停」还是「继续」，而不是自己记状态。

## 延迟测量口径

`LatencyReport` 三个字段，报告时必须说清是哪一个：

| 字段 | 含义 |
| --- | --- |
| `to_write_ms` | `publish → 回调写入`：点击到数据进 endpoint 缓冲 |
| `queued_ms` | 写入时 endpoint 已排队量，共享模式恒为 12ms，应用侧排不掉 |
| `to_audible_ms` | `publish → 出声`：前两者之和，与听感对应的那个 |

测量点位于「第一个能真正写入该 clip 的回调」。两次切换落进同一个引擎周期时，被取代的那次 publish 不会有报告——这是正确行为，不是丢数据。

## 已知未做

- 切换不做交叉淡化（架构文档 4.1 的可选项），目前是硬切
- MP4/M4A 与 OGG 尾部仍有编码器填充（956–1980 帧），只影响末尾静音
- MMCSS 线程提升是 best-effort，失败不报错也不阻塞播放
- 长文件整段解码进内存
