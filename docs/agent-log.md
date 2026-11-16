# Agent log

Shared handoff log. Newest entry on top. One entry per work session: what changed, what was verified, what is left.

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
