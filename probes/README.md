# 探针项目

支撑 `docs/architecture.md` 中所有延迟与格式数字的一次性验证程序。它们不是产品代码，不参与构建，只在需要复测时运行。

## wasapi3-probe

直接对接 `wasapi` crate 0.27，绕过 cpal，用来测量共享/独占模式的真实延迟与设备冲突行为。

```bash
cd wasapi3-probe
cargo build --release

# 列出渲染设备与默认设备
./target/release/wasapi3-probe.exe devices

# 引擎周期范围（IAudioClient3::GetSharedModeEnginePeriod）
./target/release/wasapi3-probe.exe periods "E2x2"

# 核心：publish -> 首样本进 DAC，含缓冲排队（第 6.2 节的数字来源）
./target/release/audible.exe "E2x2"

# 共享 vs 独占，同一口径（第 6.4 / 6.5 节）
./target/release/compare.exe "E2x2"

# 三种喂料策略：填满 / 补一个周期 / 空闲不喂（第 6.3 节）
./target/release/drain.exe "E2x2"

# 连续播放下的喂料策略与下溢计数（第 6.3 节后半）
./target/release/steady.exe "E2x2"

# 独占对其它客户端的影响（第 6.4 节）
./target/release/conflict.exe

# 独占模式支持哪些整数格式
./target/release/exclusive_fmt.exe "E2x2"

# 所有设备 × 格式的引擎周期扫描（第 6.1 节）
./target/release/fmtsweep.exe

# 空闲不喂料的风险检查（长空闲后流是否还活着）
./target/release/risk.exe "E2x2" 3000

# Reset / Stop+Start 能否清空排队音频（第 6.6 节，失败路径）
./target/release/switch2.exe "E2x2"
```

设备名参数是子串匹配，不区分大小写。省略则用默认渲染设备。传错名字会列出当前所有可用设备。

**注意**：默认渲染设备是虚拟声卡（FxSound）。测真实硬件要显式传 `"E2x2"` 或 `"TOPPING"`。本机当前在线的是 E2x2（真实 USB 声卡），TOPPING Pro 需要接上才有。

## index-probe

索引吞吐、格式探测、冷启动解码预算。

```bash
cd index-probe
cargo build --release

# 索引吞吐：遍历 + 元数据探测（1/2/4/8/16 线程）+ SQLite 三种写入策略
./target/release/index-probe.exe <音频库目录>

# 单文件格式探测：能拿到哪些元数据、耗时多少
./target/release/formats.exe <文件1> <文件2> ...

# 冷启动解码预算：打开 + 首包 + 到 500ms + 整文件
./target/release/coldstart.exe <文件1> <文件2> ...
```

`index-probe` 会在临时目录建三个测试数据库（`index-probe-{a,b,c}.db`），跑完自动删除。

## tauri-drag-p0

P0 验收程序：最小 Tauri 2 窗口，验证窗口内拖拽能产出真实 `CF_HDROP` 并被 DAW 接受。

```bash
cd tauri-drag-p0/src-tauri
cargo build --release
./target/release/tauri-drag-p0.exe
```

窗口列出 `samples/` 里的测试文件，按住左键拖到 DAW 的轨道区。每次拖动的开始与结果写到 exe 同目录的 `p0-drag.log`。

不用 npm、不起 dev server：`tauri.conf.json` 打开了 `custom-protocol`，静态页 `ui/index.html` 直接编进二进制，一次 `cargo build` 就出可运行的 exe。

**结论（2026-11-16）**：拖进 Cubase 16、REAPER、FL Studio 均被接受。`onEvent` 回调不触发，见 `docs/agent-log.md`。

## formats/

六种格式的同一段正弦测试音（WAV/AIFF/FLAC/MP3/OGG/M4A），由 ffmpeg 从 `tauri-drag-p0/samples/p0-tone-440-44k-stereo.wav` 转出。用于验证播放管线对每种格式的解码、重采样与声道映射。重新生成：

```bash
cd probes/formats
SRC="../tauri-drag-p0/samples/p0-tone-440-44k-stereo.wav"
for f in flac mp3 ogg m4a aiff; do ffmpeg -y -loglevel error -i "$SRC" "tone.$f"; done
```

## wav-fmt20/

复现一个 symphonia 的 WAV 头部缺陷：`fmt ` chunk 长度为 20 字节（合法的 `WAVEFORMATEX`，`cbSize = 2`）时，`symphonia-format-riff` 报 `malformed stream: wav: malformed fmt_pcm chunk`，探测拿不到任何字段。真实库 28 万文件里有 284 个是这种，全部可正常播放。

```bash
cd wav-fmt20
cargo build --release
./target/release/wave_fmt20.exe <某个 fmt chunk 为 20 字节的 wav>
```

产品代码的兜底在 `crates/mio-index/src/wav_header.rs`，只在 symphonia 拒绝时启用。

## 已知限制

- `wasapi3-probe` 的 `audible.exe` 用 `get_current_padding()` 估算「进 DAC」的时刻，不含 DAC 之前的硬件缓冲。真实感知延迟会略高。
- `index-probe` 的吞吐数据来自合成库（小 WAV、热缓存）。真实混合格式库会更慢——P2 在 28 万文件真实库上的实测是 4,247 文件/s（全量）与 5.4s（增量），见 `docs/agent-log.md`。
- ASIO 探针（第一轮）在 `%TEMP%\asio-probe`，未纳入本仓库：它需要 ASIO SDK 与 libclang，不适合随仓库分发。
