# 探针项目

支撑 `docs/architecture.md` 与 `docs/research-notes-playback.md` 中所有数字的一次性验证程序。它们不是产品代码，不参与构建，只在需要复测时运行。

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

设备名参数是子串匹配，不区分大小写。省略则用默认渲染设备。

**注意**：默认渲染设备是虚拟声卡（FxSound）。测真实硬件要显式传 `"E2x2"` 或 `"TOPPING"`。

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

## 已知限制

- `wasapi3-probe` 的 `audible.exe` 用 `get_current_padding()` 估算「进 DAC」的时刻，不含 DAC 之前的硬件缓冲。真实感知延迟会略高。
- `index-probe` 的吞吐数据来自合成库（小 WAV、热缓存）。真实混合格式库会更慢。
- ASIO 探针（第一轮）在 `%TEMP%\asio-probe`，未纳入本仓库：它需要 ASIO SDK 与 libclang，不适合随仓库分发。
