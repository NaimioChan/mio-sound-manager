# ReSlip 音频引擎

`crates/reslip-engine` 的模块细节。架构依据见 `docs/architecture.md` 第 4、8 节；本文件只说代码怎么组织、为什么这么切。实测数字见 `docs/agent-log.md` 与 `docs/architecture.md` 8.1。

## 模块

```
src/
  lib.rs      // 对外导出，模块职责一句话
  error.rs    // EngineError，解码失败带路径（UI 要显示「这个文件播不了」）
  decode.rs   // 文件 → 设备格式 PCM，可在任意线程跑
  output.rs   // WASAPI 常驻流 + 双槽调度
```

两个模块的契约是 `Arc<DecodedAudio>`：不可变、已转成设备格式、可随意共享。解码侧怎么慢都行，渲染侧只认这一份字节。

## decode

`Decoder::decode(path)` 做四件事，全部在调用线程上完成：

1. symphonia 探测 + 解码到交错 f32（用文件自己的采样率与声道数）
2. 采样率不同则 `rubato` FFT 整段重采样到设备采样率
3. 声道映射到设备声道数
4. 转成设备样本格式（f32 或 i16）的字节流

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

**槽与切换。** `slots: [ArcSwapOption<DecodedAudio>; 2]` + `active: AtomicUsize`。

`publish` 的顺序是：写入非活跃槽 → 重置位置与 finished → 打 `pub_seq` 与 `pub_ns` → 翻 `active`。渲染线程先读 `active` 再读 `pub_seq`，看到新序号就说明槽里的东西已经装好了。

旧 clip 由 `arc-swap` 负责回收：`store()` 内部会等所有 guard 释放才 drop 旧值，所以音频线程永远不会释放自己正在读的内存。**不要**自己写 retire 列表，那是重复实现而且还容易错。

**渲染回调的硬约束。** `scratch` 缓冲在循环外按 endpoint 上限分配一次；回调内只做 `copy_from_slice` 与 `fill(0)`，没有分配、没有锁、没有 IO、没有 `format!`。测量数据通过 `ArrayQueue` 送出，push 失败就丢弃——诊断数据不值得阻塞音频线程。

**`IAudioClient` 的生命周期。** 它在渲染线程内创建、使用、销毁。`set_get_eventhandle()` 返回的 `Handle` 与 `AudioClient` 同寿命；提前 drop 会关掉事件，`Start()` 报 `AUDCLNT_E_EVENTHANDLE_NOT_SET`（`0x88890014`）。

**设备格式限制。** 只支持 f32/32bit 与 i16/16bit。共享模式设备实际只发这两种；其它情况报 `Unsupported` 而不是猜字节布局。

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
