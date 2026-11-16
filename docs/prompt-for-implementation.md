# ReSlip 实现开工提示词

把下面整段复制到新对话。项目路径 `C:\Users\Naimio\projects\reslip`。

---

ReSlip 的调研和架构设计已经完成，现在要开始写代码。项目目录 `C:\Users\Naimio\projects\reslip`，先读完这几份再动手：

1. `AGENTS.md` — 技术栈、铁律、文档导航
2. `docs/architecture.md` — **主文档**。后端选型、延迟预算、线程模型、索引 schema、技术栈、实施顺序（P0–P6）、否决的路线
3. `docs/research-notes-playback.md` — 实测数据，第 6 节是 WASAPI 的关键数字
4. `docs/TODO-low-latency-audio.md` — Q1–Q6 的逐条结论

## 这次的任务

按 `docs/architecture.md` 第 8 节的实施顺序推进。**从 P0 开始**，P0 完成再动 P1。

### P0：拖拽导出可行性（必须先做完）

验收标准：从一个最小 Tauri 2 窗口把文件拖进真实 DAW（Ableton / Reaper / FL Studio 任一），DAW 接受并导入。

已知信息：
- CrabNebula 的 `drag` crate 2.1.1 有 Windows 实现（OLE + `DoDragDrop` + `CF_HDROP`），本机已验证能编译。
- `tauri-plugin-drag` 2.1.1 把它封装成 Tauri 命令 `plugin:drag|start_drag`，参数是绝对路径列表。
- **未验证的是「DAW 是否接受」这一步**，crate 编译通过不等于拖得进去。

如果 P0 失败，先停下来汇报，不要自己改外壳选型——退回 Electron 是个重大决策，要我来定。

### P1：音频引擎原型（不含 UI）

验收标准：命令行程序，指定文件路径即播，打印实测延迟；模拟连按切换无 glitch。

必须做到：
- WASAPI 共享模式常驻流（`wasapi` crate 0.27，**不用 cpal**）
- 双槽交替切换（`docs/architecture.md` 4.1）
- 音频回调里只有 memcpy，不分配不加锁不 IO 不打日志
- 支持 WAV/AIFF/FLAC/MP3/OGG/M4A 六种格式（symphonia `all-formats` + `all-codecs`）
- 打印 `publish → 出声` 延迟，能跟架构文档里的 17.5ms 对照

## 环境现状（省得你踩坑）

- Windows 11，Rust 1.99，cargo 在 `%LOCALAPPDATA%\rustup\cargo\bin`
- 本机代理 `127.0.0.1:7897`，cargo/npm/github 需要显式设 `HTTPS_PROXY` 和 `HTTP_PROXY`
- 当前默认音频设备是虚拟声卡（FxSound）。**测延迟要显式选真实硬件**（设备名子串匹配，如 `"E2x2"`），否则数字没意义
- 探针程序在 `probes/`，可直接跑，用来对照你的实现是否达到同样水平
- TOPPING Pro USB 声卡本轮没接，ASIO 相关的东西（P6）暂时做不了

## 已知的坑（都是实测踩过的）

- `IAudioClient` 是 `!Send`，不能跨线程移动，必须在用它的那个线程里创建
- `set_get_eventhandle()` 返回的 `Handle` 必须一直持有，drop 掉就等于关事件，`Start()` 报 `0x88890014`
- `Reset()` 会清掉事件句柄，crate 0.27 重新注册报 `0x8000FFFF`。别用它做「切歌清队列」
- 共享模式的引擎周期固定 10ms，`InitializeSharedAudioStream` 申请更小会被拒 `0x88890020`
- 共享模式端点缓冲里恒有 12ms 排队，应用侧排不掉。**这是 17.5ms 的主要来源，不要试图优化它**
- M4A 从头部读不到声道数（symphonia `isomp4` 返回 0ch），要解码首包才知道

## 汇报方式

每个阶段完成后更新 `docs/agent-log.md`（顶部加一条：时间/执行者/任务/改动/验证/遗留）。commit 尾部带 `Agent: hermes` 与 `Verify: <结果>` 两行。

延迟数字必须说明口径是 `publish → 回调写入` 还是 `publish → 出声`，两者差 12ms。只说「快了」不算。

---

## 附：如果想省事，可以直接复制这段起手

```
读 C:\Users\Naimio\projects\reslip 下的 AGENTS.md、docs/architecture.md，
按第 8 节的顺序开工。先做 P0：验证 Tauri 2 窗口能把文件拖进真实 DAW
（drag crate 2.1.1 已编译通过，DAW 接受性未验）。P0 过了再做 P1 音频引擎原型。
不要改外壳选型，不要动架构决策。测延迟注意默认设备是虚拟声卡，要显式选 "E2x2"。
```
