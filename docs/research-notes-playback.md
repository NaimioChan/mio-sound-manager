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

## 4. 尚未采到的数据（下一个对话要补）

- **ASIO 实际周期**：本机有 TOPPING 驱动，但 `asio-sys` 构建需要 LLVM/libclang（bindgen），参考机未安装。补测办法见 TODO 文档 Q3。
- **WASAPI 独占模式**：cpal 0.15 的 Windows 后端默认走共享模式，独占需要 `IAudioClient3` 或 WASAPI exclusive 的额外实现。未测。
- **设备端真实输出延迟**：`IAudioClient::GetStreamLatency` / `IAudioClock` 的值未取，上面的数字只到"进设备缓冲"，不含 DAC 前的一段。
- **冷启动首次 open 的开销**：探针里 `build_output_stream` 15–34ms，但那是进程刚起、设备刚被独占时的一次性成本；常驻流的稳态才是本项目的目标形态。

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
2. 之后 `bindgen` 需要 `libclang.dll`。装 LLVM 或单独提供 libclang 并设 `LIBCLANG_PATH`。
