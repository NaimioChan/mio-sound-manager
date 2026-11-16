# 调研笔记：播放路径实测（参考机）

记录日期 2026-11-16，分两轮。

- **第 1–5 节：第一轮**，用 cpal 抽象层测。数字大部分仍有效，但**第 2 节的延迟口径有误**（见该节警告）。
- **第 6 节：第二轮**，用 `wasapi` crate 直接对接，把设备缓冲的排队量单独量出来。**后端选型以第 6 节为准。**

两轮之间参考机的设备状态不同：第一轮 TOPPING Pro 接着（可测 ASIO），第二轮只接着 E2x2（测 WASAPI）。所以 ASIO 与 WASAPI 的对比数据跨了两轮，口径不完全可比，文中已分别标注。

## 参考机

- Windows 11，Rust 1.99
- 第一轮：cpal 0.15.3，symphonia 0.5.5；默认输出设备 `扬声器 (FxSound Audio Enhancer)`，F32 / 2ch / 44100Hz
- 第二轮：`wasapi` 0.27，symphonia 0.5.5；测的是 `Playback 1/2 (2- E2x2)`（真实 USB 声卡）
- 已安装 ASIO 驱动：`HKLM\SOFTWARE\ASIO\TOPPING Pro USB Audio Device`（CLSID `{5AF0A297-63D3-43D5-BCAD-838C1494D3B7}`）

## 1. 解码（symphonia，release，整文件）

| 文件 | 大小 | 时长 | 解码 |
| --- | --- | --- | --- |
| WAV 48k/2ch | 576KB | 3s | 2ms |
| FLAC 48k/2ch | 48KB | 3s | 1ms |
| MP3 320k | 122KB | 3s | 6ms |
| OGG Vorbis | 69KB | 20s | 29ms |
| WAV 44.1k | 5.3MB | 30s | 17ms |
| WAV 96k/24bit | 104MB | 3min | 209ms |

含义：对采样管理器常见的 1–10 秒素材，**整文件解码**只要 1–6ms。只有超大文件（分多分钟的现场录音、长循环）才需要流式/分块策略。

> 第二轮补测的是**另一个指标**：从打开文件到拿到**第一个包**（不是整文件），7 种格式全部 < 0.4ms，见 3.1 节。冷启动关心的是这个数，不是整文件解码时间。两个数不冲突，是不同场景。

## 2. WASAPI 共享模式输出

> **⚠️ 本节的数字口径有误，已被第 6 节取代。** 这里测的是「发布 → 回调第一次取到它」，没有计入设备缓冲里已排队的音频。真实点击到出声是 **17.5ms**（第 6.2 节）。保留本节是为了说明当时的误判过程，不要引用这里的 8.33ms。

常驻流，测"发布新片段 → 回调第一次取到它"：

```
swap -> first render: median 8.7-9.4 ms, min 0.01-0.06, max 10.2-10.5   (三轮独立测量)
samples: [0.06,0.07,0.08,0.09,0.09,0.09,0.09,0.1,0.17,8.6,8.66,8.77,9.3,
          9.31,9.57,9.68,9.74,9.8,9.91,10.19]
```

双峰：撞上回调边缘时 <1ms，否则等一个完整周期（8–10.5ms）。

回调几何由驱动固定，**请求更小的 buffer 无效**：

```
request=None -> 446 frames/callback (10.12 ms)
request=512  -> 446-470 frames (10.1-10.7 ms)
request=256  -> 446 frames (10.12 ms)
request=128  -> 446 frames (10.12 ms)
request=64   -> 446-485 frames (10.1-11.0 ms, 偶有 882 帧抖动块)
request=32   -> 446 frames (10.12 ms)
```

含义：共享模式下限约 10ms + 设备输出延迟。要再低必须换独占或 ASIO。

## 3. webview 媒体元素（对照，用于确认换壳无收益）

同页同文件，Electron 39 与 Edge 157（= WebView2 内核）：

| 环境 | 复用元素换 src | 预载好的另一个元素 |
| --- | --- | --- |
| Electron 39 | 中位 39ms | 0ms |
| Edge 157 | 中位 32ms | 0ms |

含义：换 Tauri 不改变 webview 内播放延迟；同时说明"预载好 → play()"能把延迟压到测量噪声以下，这是 webview 路径的最强项。

## 4. ASIO 实测（第一轮，口径与第 6 节不同）

> **⚠️ 本节的「发布→出声」列与第 6 节不可比。** 它计到「回调把数据交给驱动」，不含驱动/设备内部缓冲。第 6 节用 `get_current_padding()` 把排队量算进去了。两个数字不能直接相减或相比。

`TOPPING Pro USB Audio Device` 驱动报告的 buffer 范围是 **8–2048 帧**，cpal 接受 `Fixed()`，回调块精确无抖动（64/64、128/128、256/256）。配置被自动协商到 `I32, 1 ch, 44100 Hz`（cpal 的 ASIO 默认取第一组可用配置）。

| 后端 | 请求 buffer | 实际回调周期 | 发布→出声 中位 | min | p90 | max |
| --- | --- | --- | --- | --- | --- | --- |
| WASAPI 共享 | 默认（450 帧） | 10.20 ms | 8.33 ms | 0.05 | 10.05 | 10.32 |
| **ASIO** | **64 帧** | **1.45 ms** | **0.68 ms** | 0.17 | 1.16 | 1.47 |
| **ASIO** | **128 帧** | **2.90 ms** | **0.61 ms** | 0.36 | 2.14 | 2.42 |
| ASIO | 256 帧 | 5.80 ms | 3.68 ms | 0.01 | 3.98 | 4.53 |

**ASIO 64 帧比 WASAPI 共享快约 12 倍**（0.68ms vs 8.33ms），且完全可预测：回调率 687/s，块大小稳定 64 帧。（两个数字同口径，这个倍数成立；但它们都不等于真实出声延迟。）

### ASIO 不是独占的（决定性发现）

用两个进程同时开 ASIO 流（模拟"DAW 已占用设备"）：两个实例都正常出声，第二个实例的回调率 **690/s**、块大小仍稳定 64 帧、发布→出声中位 **0.69ms**，与单实例（0.68ms）无差异。

含义：**ASIO 在 cpal 这条路径上允许多客户端共享**（TOPPING 驱动实现了多 client 支持）。"开着 DAW 时采样器变哑"这个最担心的失败模式在本机不成立。这大幅降低了选 ASIO 的风险——但仍需在真实 DAW（Ableton/Reaper 用 ASIO 独占缓冲）下复验，因为 cpal 的两个实例和"DAW + 采样器"不是完全等价的场景。

含义（续）：
- **设备周期不再是瓶颈**。剩下的下限由解码首段 + 进程内调度决定，这两项都在可控范围内（短文件解码 1–6ms）。
- 这直接把 Q1 的 `< 15ms` 目标从"紧张"变成"宽裕"——即使算上 6ms 解码，总预算也只用了约 7ms 的一半。
- 代价是 1 声道（cpal 选到的配置）。实际产品需要显式协商 2 声道配置，这件事要在 Q3 里确认 `TOPPING` 是否支持（它报告 6 声道能力，但 cpal 的候选列表里只挑到了 1ch）。

ASIO 构建的两个坑（已踩过，写下来免得重踩）：

1. `asio-sys` 的 build.rs 会下载 Steinberg ASIO SDK 到系统临时目录再 `rename` 成 `asio_sdk`，在 Windows 上常因目录被占用失败。绕过办法：手动把 SDK 解压到 `%TEMP%\asio_sdk`（顶层目录名必须是 `asio_sdk`，里面要有 `host`、`common`、`host/pc`），build.rs 检测到它就跳过下载。
2. 之后 `bindgen` 需要 `libclang.dll`。**不必装整套 LLVM**：`pip install libclang` 会附带一个 `libclang.dll`（本机在 `.../site-packages/clang/native/`，约 60MB），把它设进 `LIBCLANG_PATH` 即可。
3. ASIO 的 `default_output_config()` 常常不可用；要遍历 `supported_output_configs()` 逐个试建流，并按 `sample_format()` 分派不同的闭包类型（f32/i16/i32/u16 各一份）。

## 5. 尚未采到的数据

> 本节是**第一轮**的遗留清单。第 6 节已经补掉了「WASAPI 独占模式」「设备端排队」两项，并新增了设备冲突结论。仍然缺的是：ASIO 立体声配置、ASIO 真实出声延迟、真实 DAW 占用场景。

- ~~**WASAPI 独占模式**~~ → 已测，见 6.4。3ms 周期、4.5ms 出声，但会抢设备
- ~~**设备端真实输出延迟**~~ → 已测排队量，见 6.2。共享模式恒定 12ms
- **ASIO 立体声配置**：第一轮只跑到 1 声道，需要确认 2ch 是否可用及是否影响延迟。
- **ASIO 真实出声延迟**：需要用 `get_current_padding()` 同口径复测（本轮 TOPPING 未接）。
- **ASIO 设备独占时 DAW 的行为**：需要开着真实 DAW 再跑探针。
- **冷启动首次 open 的开销**：`build_output_stream` 15–34ms 是进程刚起时的一次性成本；常驻流的稳态才是目标形态。

## 复现

第一轮（已过时，仅存档）：

```bash
# 解码 + WASAPI 常驻流（第一轮数据来源）
cd <临时目录>/rust-audio-probe
cargo run --release            # 常驻流 swap 延迟
cargo run --release --bin geometry     # 回调几何
cargo run --release --bin decode_probe -- <音频文件>

# ASIO（需先解决 libclang）
cd <临时目录>/asio-probe
cargo build --release && ./target/release/asio-probe.exe 20
```

第二轮（第 6 节数据来源）见 6.7，探针已收进仓库 `probes/`。

第一轮的探针项目在 `%TEMP%\rust-audio-probe` 与 `%TEMP%\asio-probe`，未进仓库（ASIO 那个需要 SDK + libclang）。重建依赖：`cpal = { version = "0.15", features = ["asio"] }`、`symphonia = "0.5"`、`hound = "3.5"`。

ASIO 构建的两个坑（已踩过，写下来免得重踩）：

1. `asio-sys` 的 build.rs 会下载 Steinberg ASIO SDK 到系统临时目录再 `rename` 成 `asio_sdk`，在 Windows 上常因目录被占用失败。绕过办法：手动把 SDK 解压到 `%TEMP%\asio_sdk`（顶层目录名必须是 `asio_sdk`，里面要有 `host`、`common`、`host/pc`），build.rs 检测到它就跳过下载。
2. 之后 `bindgen` 需要 `libclang.dll`。**不必装整套 LLVM**：`pip install libclang` 会附带一个 `libclang.dll`（本机在 `.../site-packages/clang/native/`，约 60MB），把它设进 `LIBCLANG_PATH` 即可。
3. ASIO 的 `default_output_config()` 常常不可用；要遍历 `supported_output_configs()` 逐个试建流，并按 `sample_format()` 分派不同的闭包类型（f32/i16/i32/u16 各一份）。

## 6. WASAPI 的重新实测（2026-11-16 第二轮，纠正第 2 节的口径）

第 2 节的数字只计到「回调把这一块写进设备缓冲」，**没有算缓冲里已经排在前面的音频**。真实的点击到出声必须把这段排队时间加上。本节用 `wasapi` crate 0.27 直接对接 `IAudioClient` / `IAudioClient3`，用 `get_current_padding()` 把排队量单独量出来。

参考设备：`Playback 1/2 (2- E2x2)`（真实 USB 声卡，44.1k / 2ch / f32）。TOPPING Pro 本轮未接，因此 ASIO 无法用同一方法复测。

### 6.1 共享模式：引擎周期固定 10ms，且不可调

`IAudioClient3::GetSharedModeEnginePeriod` 在三台在线设备（E2x2、Realtek、FxSound 虚拟声卡）上返回的 `min` / `default` / `max` **全部相等，等于 10.00ms**：

```
E2x2     : default=441 fundamental=441 min=441 max=441 frames (10.00ms)
Realtek  : default=480 fundamental=480 min=480 max=480 frames (10.00ms)
FxSound  : default=441 fundamental=441 min=441 max=441 frames (10.00ms)
```

扫过 f32 / i16 / i24-in-32 × 44100 / 48000 / 96000 × 2ch 的全部组合，结论一致：**只要设备接受该格式，引擎周期就是 10ms，换格式无效。** 调 `InitializeSharedAudioStream` 请求 `2 × fundamental`（882 帧）会被拒绝（`0x88890020`）。

含义：Windows 10+ 上 `IAudioClient3` 能压低共享模式周期的说法，在本机三台设备上都不成立。这个下限来自驱动/引擎，不是 API 用法问题。

### 6.2 共享模式的真实延迟：17.5ms，不是 8.33ms

常驻流，每 50ms 发布一次「切换片段」，同时记录写入时刻的排队深度：

```
端点缓冲 970 帧 (22.00ms)
写入时队列深度: 中位 529 帧 (12.00ms)，恒定不抖
publish -> 回调写入:  中位 4.71ms (min 0.26, p90 8.60, max 9.84)
  其中已排在前面:     中位 12.00ms (恒定)
publish -> 首样本进 DAC: 中位 17.5ms (min 12.0, p90 20.9, max 21.9)   三轮独立复测
```

`autoconvert` 开关、请求 5/10/20ms 缓冲，都改变不了 22ms 端点缓冲和 12ms 排队：引擎自己管理这个预滚，应用侧看不进去也排不掉。

**这是对第 2 节 8.33ms 的修正。** 8.33ms 是「等一个回调周期」的均值，真实点击到出声还要再加恒定 12ms 排队。

### 6.3 空闲不喂料可以把队列排空，但会下溢静音

空闲时**完全不写**（让引擎自行填静音），队列降到 0，切换延迟掉到 5.6ms：

```
policy FillAll   : padding 中位 529 帧 | publish->DAC 中位 17.14ms
policy DrainIdle : padding 中位 529 帧 | publish->DAC 中位 17.14ms
policy StarveIdle: padding 中位   0 帧 | publish->DAC 中位  5.56ms
```

代价是连续播放时每两个回调就有一次下溢（实测 250/252 次回调 padding 为 0），也就是音频被静音填充，听感是断续。因此 5.6ms 只适用于「切一下就停」的试听，不能用于连续播放一个 20 秒 loop。

### 6.4 独占模式：3ms 周期、4.5ms 出声，但会让别的程序静音

独占模式要求整数格式，浮点 mix format 被拒（`0x88890008`）。E2x2 只接受 16bit 与 24-in-32、且只在 44100Hz：

```
[exclusive 16bit 44k1]  period 3.00ms -> buffer 132 帧 (2.99ms)
  publish->DAC: 中位 4.42ms (min 3.00, p90 5.57, max 5.74)
[exclusive 24in32 44k1] period 3.00ms -> buffer 132 帧 (2.99ms)
  publish->DAC: 中位 4.41ms (min 3.10, p90 5.59, max 5.91)
```

设备冲突已实测（持有独占后再开客户端）：

```
second exclusive client: REFUSED (0x8889000A)
shared client while we hold exclusive: REFUSED (0x8889000A)
```

**结论：WASAPI 独占会让用户开着的 DAW / 浏览器 / 播放器全部静音。** 对采样管理器这是硬伤，与之前对 ASIO 的担心方向相反——真正会抢设备的是独占模式，不是 ASIO（ASIO 在本机驱动上已验证多客户端）。

### 6.5 三种后端的同一口径对比

| 后端 | 模式 | 设备周期 | publish→写入 | 排队 | **publish→出声（中位）** | 抢设备 |
| --- | --- | --- | --- | --- | --- | --- |
| WASAPI 共享 | event | 10.00ms 固定 | 4.7ms | 12.0ms | **17.5ms** | 否 |
| WASAPI 共享（空闲不喂） | event | 10.00ms | — | 0 | **5.6ms**（连续播放会下溢） | 否 |
| WASAPI 独占 | event | 3.00ms | — | 3.0ms | **4.5ms** | **是** |
| ASIO 64 帧 | — | 1.45ms | 0.68ms | ≤1.45ms | **约 2ms**（未用同一方法复测） | 否（本机驱动多客户端） |

ASIO 那一行是第 4 节的数字加一个 ASIO 缓冲的保守估计，本轮 TOPPING 未接，无法用排队法复测，标为待验。

### 6.6 队列清空的尝试（失败，记录以免重试）

想在切换时丢掉排队音频，试过 `Reset()`：

- 流运行中调用 `Reset()` 被拒：`0x88890005`（AUDCLNT_E_NOT_STOPPED），必须先 `Stop()`。
- `Stop()` → `Reset()` → `Start()` 之后 `Start()` 报 `0x88890014`（AUDCLNT_E_EVENTHANDLE_NOT_SET）：`Reset()` 会清掉注册的事件句柄。
- 重新 `set_get_eventhandle()` 报 `0x8000FFFF`（E_UNEXPECTED），`wasapi` crate 0.27 这条路径走不通。
- 单纯 `Stop()` + `Start()` 不清队列：padding 仍为 529 帧，延迟仍 18.3ms。

含义：**共享模式下排队时间在流生命周期内无法丢弃。** 要绕过只能在每次切换时重建整条流（`build_output_stream` 一次性成本 15–34ms，得不偿失）。

### 6.7 复现

```bash
cd probes/wasapi3-probe          # wasapi 0.27，本轮数据来源
cargo build --release
./target/release/wasapi3-probe.exe devices
./target/release/wasapi3-probe.exe periods "E2x2"    # 引擎周期范围
./target/release/audible.exe "E2x2"                  # 排队 + 真实出声
./target/release/compare.exe "E2x2"                  # 共享 vs 独占，同口径
./target/release/drain.exe "E2x2"                    # 三种喂料策略
./target/release/steady.exe "E2x2"                   # 连续播放下的策略
./target/release/conflict.exe                        # 独占对其它客户端的影响
./target/release/switch2.exe "E2x2"                  # 队列清空尝试
```

探针已收进仓库 `probes/`，详见 `probes/README.md`。

