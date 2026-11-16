# ReSlip 低延迟播放调研 — 启动提示词

把下面整段复制到新对话。项目路径 `C:\Users\Naimio\projects\reslip`。

---

我正在做一个叫 ReSlip 的项目：把 Slip Sound（一个本地音频采样管理器，Electron + SolidJS）的后端用 Rust + Tauri 重写。UI/UX 和交互模型已经确定、照搬现有版本，我不需要重新设计界面。我要解决的是后端两个硬指标：**索引一个大型采样库要足够快**，以及**点任意一个音频文件到出声的延迟要足够低**。

项目目录 `C:\Users\Naimio\projects\reslip` 已经建好，里面有三份必读文档，请先全部读完再开始：

1. `AGENTS.md` — 技术栈、项目铁律、文档边界
2. `docs/research-notes-playback.md` — 我已经在参考机上实测的数据（解码耗时、WASAPI 周期、webview 对照）
3. `docs/TODO-low-latency-audio.md` — **这次调研的工作清单**，六个待回答问题（Q1–Q6），每个都写了要回答什么、要产出什么

## 这次的任务

按 TODO 文档做调研，重点是 **Q1 和 Q3**：

- **Q1**：常驻音频流 + 分段喂料的模型，能不能让"从未解码过的任意格式文件"的第一声不依赖整个解码完成？给出 cold-to-first-sample 的逐段延迟预算分解。目标 < 15ms，如果不可达请说清实际下限和构成。
- **Q3**：输出后端到底选 ASIO、WASAPI 独占还是 WASAPI 共享。需要实测，不能只查文档。

**不要写实现**。这次要的是结论和架构决策，不是可用的后端。可以写一次性验证原型（能跑、能打印耗时数据即可），但不要开始搭产品代码结构。

## 明确约束

- **排除 Web Audio**（`decodeAudioData` + `AudioBufferSourceNode`）。这条已经验证过不合适：它把播放锁在 Chromium 音频线程和 AudioContext 时钟上，无法用 ASIO，也无法控制设备周期。
- **允许用 ASIO**。我的机器上装了 `TOPPING Pro USB Audio Device` 的 ASIO 驱动（`HKLM\SOFTWARE\ASIO`）。但要评估独占设备的代价——我平时开着 DAW，如果 ReSlip 用 ASIO 独占导致 DAW 开着时它变哑，这个方案可能不能接受。这点请给明确判断，不要含糊。
- **任意格式**都要覆盖冷路径：WAV、AIFF、FLAC、MP3、OGG Vorbis、M4A/AAC。不接受"先统一转码"的方案。
- 目标形态是**常驻服务**：进程启动时打开一次音频流，之后切文件不重开设备。

## 环境现状（省得你踩坑）

- Windows 11，Rust 1.99，cargo 可用
- 参考机**没有装 LLVM/libclang**，而 `asio-sys` 的 build.rs 需要它（bindgen）。这是 Q3 的第一个阻塞，需要先解决：装 LLVM，或者找绕过 bindgen 的办法。ASIO SDK 我已经手动解压到 `%TEMP%\asio_sdk`（绕过了 asio-sys 在 Windows 上 rename 失败的问题），顶层目录名必须是 `asio_sdk`，里面要有 `host`、`common`、`host/pc`。
- 之前实测用的临时探针项目在 `%TEMP%\rust-audio-probe`（cpal/symphonia 解码与 WASAPI 常驻流）和 `%TEMP%\asio-probe`（ASIO，未编译成功）。可以直接复用或重建。
- 默认输出设备是 `扬声器 (FxSound Audio Enhancer)`，这是个**虚拟声卡**，测出来的数字不代表直连 USB 声卡。测真实延迟时请确认走的是 TOPPING 或 WASAPI 独占路径，别被虚拟设备误导。

## 汇报方式

调研完成后，请更新 `docs/TODO-low-latency-audio.md` 里每个问题的答案（保留原问题，答案写在下面），并按文档里"交付物"一节产出：

1. 架构决策文档（输出后端选型 + 理由 + 回退方案）
2. 延迟预算表（ASIO / WASAPI 独占 / 共享三种模式的逐段实测）
3. 索引性能报告（17 万文件端到端 + 各阶段占比）
4. 可运行的最小验证原型 + 每段耗时打印
5. 明确列出否决的路线和依据

索引部分（Q4）可以和播放部分并行调研，它们互不阻塞。如果播放这块卡在环境问题上（比如装 LLVM 失败），先把 Q4 做完，并明确记录播放侧的阻塞点。

我关心的是**数字和取舍**，不是综述。每个"可行"的结论都要有实测支撑。

---

## 附：如果想省事，可以直接复制这段起手

```
读 C:\Users\Naimio\projects\reslip 下的 AGENTS.md、docs/research-notes-playback.md、
docs/TODO-low-latency-audio.md，然后按 TODO 做调研，重点 Q1 和 Q3。
不要写实现，允许写一次性验证原型。ASIO 需要先解决 libclang 缺失。
测延迟时确认走真实声卡而不是 FxSound 虚拟设备。
```
