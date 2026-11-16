# 调研笔记：播放路径实测（参考机）

记录日期 2026-11-16。这些数字用于给 ReSlip 定立延迟目标，**不是**最终结论：参考机的默认输出设备是虚拟声卡（FxSound Audio Enhancer），真实 ASIO 声卡（TOPPING Pro USB Audio Device）的数据尚未采到。

## 参考机

- Windows 11，Rust 1.99，cpal 0.15.3，symphonia 0.5.5
- 默认输出设备：`扬声器 (FxSound Audio Enhancer)`，F32 / 2ch / 44100Hz
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

含义：对采样管理器常见的 1–10 秒素材，**解码不是瓶颈**（1–6ms）。只有超大文件（分多分钟的现场录音、长循环）才需要流式/分块策略。这决定了后端可以有条件地"整文件解码进缓存"，而不是一律流式。

## 2. WASAPI 共享模式输出

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

## 4. ASIO 实测（本轮最重要数据）

`TOPPING Pro USB Audio Device` 驱动报告的 buffer 范围是 **8–2048 帧**，cpal 接受 `Fixed()`，回调块精确无抖动（64/64、128/128、256/256）。配置被自动协商到 `I32, 1 ch, 44100 Hz`（cpal 的 ASIO 默认取第一组可用配置）。

| 后端 | 请求 buffer | 实际回调周期 | 发布→出声 中位 | min | p90 | max |
| --- | --- | --- | --- | --- | --- | --- |
| WASAPI 共享 | 默认（450 帧） | 10.20 ms | 8.33 ms | 0.05 | 10.05 | 10.32 |
| **ASIO** | **64 帧** | **1.45 ms** | **0.68 ms** | 0.17 | 1.16 | 1.47 |
| **ASIO** | **128 帧** | **2.90 ms** | **0.61 ms** | 0.36 | 2.14 | 2.42 |
| ASIO | 256 帧 | 5.80 ms | 3.68 ms | 0.01 | 3.98 | 4.53 |

**ASIO 64 帧比 WASAPI 共享快约 12 倍**（0.68ms vs 8.33ms），且完全可预测：回调率 687/s，块大小稳定 64 帧。

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

## 5. 尚未采到的数据（下一个对话要补）

- **ASIO 立体声配置**：本轮只跑到 1 声道，需要确认 2ch 是否可用及是否影响延迟。
- **ASIO 设备独占时 DAW 的行为**：这是 Q3 的关键取舍，未测。需要开着 DAW 再跑探针，看 ASIO 是否被抢走。
- **WASAPI 独占模式**：cpal 0.15 的 Windows 后端默认走共享模式，独占需要 `IAudioClient3` 或额外实现，未测。
- **设备端真实输出延迟**：`IAudioClient::GetStreamLatency` / `IAudioClock` 的值未取；上面的数字只到"进设备缓冲"，不含 DAC 前的一段。ASIO 同理（`ASIOGetLatency`）。
- **冷启动首次 open 的开销**：`build_output_stream` 15–34ms 是进程刚起时的一次性成本；常驻流的稳态才是目标形态（ASIO 的 `play()` 几乎瞬时，0–13us）。

## 复现

```bash
# 解码 + WASAPI 常驻流（本轮数据来源）
cd <临时目录>/rust-audio-probe
cargo run --release            # 常驻流 swap 延迟
cargo run --release --bin geometry     # 回调几何
cargo run --release --bin decode_probe -- <音频文件>

# ASIO（需先解决 libclang）
cd <临时目录>/asio-probe
cargo build --release && ./target/release/asio-probe.exe 20
```

探针项目都是临时的，未进仓库。重建依赖：`cpal = { version = "0.15", features = ["asio"] }`、`symphonia = "0.5"`、`hound = "3.5"`。

ASIO 构建的两个坑（已踩过，写下来免得重踩）：

1. `asio-sys` 的 build.rs 会下载 Steinberg ASIO SDK 到系统临时目录再 `rename` 成 `asio_sdk`，在 Windows 上常因目录被占用失败。绕过办法：手动把 SDK 解压到 `%TEMP%\asio_sdk`（顶层目录名必须是 `asio_sdk`，里面要有 `host`、`common`、`host/pc`），build.rs 检测到它就跳过下载。
2. 之后 `bindgen` 需要 `libclang.dll`。装 LLVM 或单独提供 libclang 并设 `LIBCLANG_PATH`。**最省的办法是 `pip install libclang`**，它带的 DLL 就够用（约 60MB，本机路径 `.../site-packages/clang/native/`），不用装整套 LLVM。
3. cpal 的 ASIO host 常常拒绝 `default_output_config()`。必须遍历 `supported_output_configs()` 逐个试，并按 `sample_format()` 分派不同的闭包类型（f32/i16/i32/u16 各一份）。
