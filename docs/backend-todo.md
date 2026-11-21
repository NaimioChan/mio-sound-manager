# 后端改进项 TODO

来源：2026-11-20 的源码通读、外部调研（WASAPI / symphonia / FTS5 / 同域索引项目）与一份项目外的搜索引擎咨询比对。按优先级排序；每项注明依据与验证方式。做完的划掉并记入 `docs/agent-log.md`。

## 1. [x] prune 的盘掉线保护（健壮性，最高优先）

- 现状：`prune_missing` 在全量 pass 时删除库里所有 walk 没见到的路径。若移动硬盘掉线 / 根目录打不开，walkdir 吞掉目录级 IO 错误，结果是「全量重索引后库变空 + 28 万行被正确地删光」。
- 改法：walk 阶段统计 IO 错误数；根目录 open 失败或错误数超阈值（建议：任意一级目录枚举失败即计数，>0 就值得警惕）时中止本次 pass、报错给 UI，不进入 prune。
- 验证：集成测试——mock 一个中途消失的目录，断言库内容不变且返回错误。
- 完成：根目录枚举失败在打开数据库前报错；子目录遍历错误向上传播，任何错误都不会进入写入或 prune。缺失根目录回归测试断言不会新建空库。
- 验证补齐：扫描回调注入目录消失错误并确认原索引行不变；UI 的阻塞索引命令将 `AppError` 拒绝传回前端，`addLibrary`/`reindexLibrary` catch 会在 notice 中显示错误。后台轮询式 reindex 仍只写日志；当前产品 UI 使用的是阻塞 `reindex_library`。

## 2. [x] 预解码邻行与长素材播放（仅 >60s 文件）

- 现状：`docs/architecture.md` 3.2/3.4 承诺对 >60s 的文件预解码邻行（±2 行、首 5 秒、约 8MB 池），代码里只有 `last_clip` 缓存当前 clip，没有预取。
- 收益：大文件（3 分钟 96k）冷启动从 ~130ms 拉回 ~17.5ms。
- 改法：decode worker（`LatestSlot`）旁边加预取队列：`play` 命中列表行时异步预解码上/下各 2 行的首 5 秒，LRU 上限 4 条目（≈8MB，并进 256MB 预算）。
- 验证：mio-cli 增加连按模拟；CDP `check-interactions` 的点击→波形口径。
- 完成：UI 播放当前行时把已加载结果页 ±2 邻行交给容量 4 的有界队列；只有探测时长 >60s 时解码首 5s，短文件预取完整 clip；FIFO 队列容量 4，pending 路径去重，LRU 最多 4 个邻行项 + 当前项。256MiB 是单 clip retained PCM 上限及 LRU 条目的记账上限，不是进程峰值内存上限：重采样临时缓冲和引擎双槽持有的旧 clip 尚未纳入总量约束。超过单 clip PCM 上限的完整解码会被拒绝，因此超长素材的前缀接续和任意位置 seek 尚未完成。
- 验证：IPC 测试用 65 秒 WAV 验证 5 秒前缀、完整时长和接续；`check-layout.mjs`、`check-volume.mjs` 通过。`check-interactions.mjs` 在 22 分钟真实库素材上暴露未完成项：点波形 seek 到 70% 后位置仍停在约 1.43s。此类 seek 会请求完整解码；若 PCM 超过 256MiB 限制，无法发布，长素材需要分段 seek/decode 与 render handoff。未做人耳听感验收。
- 收尾完成（2026-11-21）：长素材改为**窗口链**——引擎 `Decoder::with_window` 按 trimmed-timeline 丢/裁包头实现任意起点的有界解码（输出与全量解码逐样本对齐，WAV/MP3/MP4 三个 demuxer 的 seek 语义逐一核实）；引擎双槽旁加一个排队槽，render 回调在当前 buffer 耗尽的同一次回调里原子接管下一窗口（render 段交接，无轮询间隙）；应用层 `play`（含 seek 目标）发布首窗口（15s），`mio-decode-ahead` 线程逐窗口解码并排队直到文件完结，256MiB 预算按「两窗口 + LRU」天然满足。`pos_frames` 保持 buffer 坐标，`snapshot` 加 `window_start` 输出文件坐标。验证：engine 窗口逐样本对齐测试、IPC 90s WAV seek 40s 落位/顺播/时长测试、`check-interactions.mjs` 在 22 分钟素材上点波形 70% 后播放头 800.07s 且顺播前进（此前停在 1.43s）。未做人耳听感验收；队列深 1，解码慢于播放时回调写静音至下一窗口就位。

## 3. [x] symphonia `opt-simd` feature 检查（性能，一行改动可能白拿收益）

- 依据：symphonia 官方 BENCHMARKS.md 用 `-C target-cpu=native` + SIMD 跑出与 FFmpeg ±15% 的数字；SIMD feature（`opt-simd-sse`/`-avx`，默认开；但 0.5.x 曾默认关，需核实当前版本）未开时 AAC 慢 10–27%。
- 改法：检查 `crates/mio-engine/Cargo.toml` 与锁定的 symphonia 版本文档；缺就补 `opt-simd`。
- 验证：`cargo run --release -p mio-cli -- probe-bench <folder> --limit 20000` 前后对照。
- 完成：锁定版本 0.5.5 未启用该 feature，engine 依赖已开启。feature tree 与编译通过；未单独跑 AAC probe-bench 前后对照，因此不宣称解码收益。

## 4. [x] 波形与播放复用同一次解码（有界缓存）

- 现状：点一个文件，`play` 解码一次（设备格式），`get_waveform` 又 `decode_source` 解一次。3 分钟 96k 文件 = 2×112ms。
- 改法：worker 里缓存刚解出的 `SourceAudio`（按 path 键），`get_waveform` 命中则跳过解码；缓存纳入 256MB LRU。
- 验证：`get_waveform` 返回的 `decodeMs` 对照（命中应近 0）。
- 完成：`Decoder::decode_with_source` 单次读/解码产出设备格式及可选 `Arc<SourceAudio>`；播放 clip 持源 PCM 时 waveform 直接生成包络，cache hit 报 `decodeMs=0`。邻行 LRU 最多 4 个邻项 + 当前项，按设备 PCM + 源 PCM 字节计量，整体 256MiB；当前播放 clip 用弱索引，缓存强引用由 LRU 管理。
- 验证：IPC 测试增加播放后 waveform 命中断言；完整 IPC/workspace 测试通过。

## 5. [x] trigram 子串搜索（按高频需求实现）

- 缺口：现在只能按词的前缀搜；搜 `iano` 找不到 `Piano_C4.wav`。
- 方案：FTS5 辅助表 `tokenize="trigram"`（不替换现有 unicode61 表——trigram 无法表达词级前缀查询，<3 字符查不到），主查询空结果时回退。
- 代价：索引体积增加（量级需实测）、写入变慢。
- 完成：按用户确认为高频需求实现；现有 unicode61 前缀表保留，主查询空结果且查询长度至少 3 字符时回退到 filename-only trigram 表；count/search 共用过滤条件，插入/更新/删除触发器维护，旧库首次打开 rebuild 并写版本元数据。
- 验证：测试 `iano` 命中 `Piano_C4.wav`、前缀优先、1–2 字符限制、更新/删除与 count 一致。真实库 DB 从 297,840,640 增至 333,385,728 字节，约增加 35.5MB（11.9%）；这是 trigram 空间成本，首次构建包含 migration/rebuild 时间，尚未与后续 incremental 写入单独对比。

## 6. [x] 分类器换 Aho-Corasick（分类等价，性能未加速）

- 现状：每文件多次 HashMap 查找 + 短语表线性扫描；分类占索引总耗时约 17%。
- 收益上限：全量 68s → ~58s（瓶颈是探测的随机读 IO，占 80%）。
- 改法：`aho-corasick` crate 用一个 overlapping automaton 扫归一化路径；curated/UCS 精确词、经原 stem 规则验证的词形变体、两套短语共用一次扫描。token span 只接受完整 token 命中；同 token exact 优先 stem；品牌/目录投票与 curated→UCS 两阶段结构保留。
- 优先级说明：排在功能项之后，库规模上去再动。
- 验证：80 个 index tests 通过，覆盖 exact-over-stem 与重叠短语；真实库 282,762 行 `classify-diff` category/subcategory/confidence/matched terms 全 **0 mismatch**。`classify-bench` checksum `1,091,774`；one-pass Aho 约 2.73–3.71s，对比 HashMap 基线 2.67s，略慢且受 Windows 负载/页缓存波动；按用户指定保留 AC 架构，不宣称性能收益。

## 7. [x] 增量 diff 改 SQLite staging + 有序批次流

- 现状：每次增量索引 `SELECT path, mtime_ns` 建 28 万条目的 HashMap（O(库) 内存，约 1–2s）。
- 改法：walk 输出写入 FILE-backed SQLite TEMP staging，按 path 游标分页归并比较，内存 O(batch)。旁证：reflect-sync 等同域项目明确以 SQLite 元数据替代内存图。
- 验证：增量 pass 计时（现基线 5.4s）与内存峰值。
- 完成：walk 逐项写 bounded batch 到 SQLite temp staging；完整 walk 成功后以路径游标/批次和既有声音表比较，只在 O(batch) 内存中构造 probe/write rows；prune 直接 NOT EXISTS staging。
- 验证：真实 282,762 行多次 hot incremental wall 约 4–20s，波动显著；最近 RAM-backed staging 约 11.6s wall / 10.9s stage，对比历史 5.4s 基线未稳定胜出。现已切到 FILE-backed temp staging 避免隐性 O(库) RAM；改后的最终计时/RSS 尚未验证，故只完成内存复杂度目标，不声称更快。

## 8. 小项

- [x] `summary()` 用 `idx_favorite` 覆盖扫描求 favorite/total；fallback 在 bulk 暂无此索引时走普通表计数。
- [x] `folder_tree` 按 library root 的 `index_version` 缓存；声音 upsert/prune 与版本 bump 在同一 SQLite transaction 中提交。
- [x] 解码 packet loop 加 30s wall-clock deadline，IPC 最长等 35s；超时按 size+mtime fingerprint 记住，文件变化后可重试。超时只在 packet 边界检查，单次阻塞文件系统调用无法被 Rust 线程强制取消。
- [x] M4A priming 哨兵补负例测试（首段有真实瞬态不该被裁）。
- [x] `exports/` 启动清理：超过 7 天删除，超过 512MiB 后按最旧时间淘汰；有容量淘汰回归测试。
- [x] 多实例同库检测：Windows 对 canonical-root 派生的 temp lock 文件使用禁止共享打开；错误说明库已被另一 Mio 实例打开；跨进程 lock 测试通过。
- [x] 清掉 `History::clear` dead-code；非 ASIO build 的 `probe_asio` 配置警告用 cfg allow 处理。
- [x] 超长素材的有界窗口 seek 与连续播放：完成（2026-11-21，见第 2 项收尾）。`check-interactions.mjs` 在 22 分钟真实库素材上点波形 70% 位置后播放头落在 800.07s 并顺播前进（此前停在 1.43s）；65 秒以上素材的 seek 落位、顺播无停顿由 IPC 测试 `long_file_seek_to_an_undecoded_position_plays_from_there` 覆盖；内存上限按「两窗口（各 15s）+ LRU」满足。未做人耳听感验收。

## 已调研并关闭（勿重试）

- WASAPI 共享模式再压延迟：驱动不给小周期就是无解（Realtek 等锁死 10ms）；且申请小周期会把同端点所有共享流一起切换，会改用户 DAW 的缓冲行为——不该做。`wasapi_relink` 项目印证。
- FTS5 `detail=none`：丢 column filter，rank 不可用，不是纯赢，撤回。`prefix='2 3'` 索引仅当真实库出现慢查询案例再做（Mozilla ADR 实测最坏 ~1ms，大概率不值得）。
- Tantivy / 自建 FST + Roaring 搜索引擎：查询 65ms 离卡顿两个量级，引入引擎依赖不划算。
- WASAPI 独占、空队列排延迟、`Reset()`、`NOT IN`、8 线程上限：此前已实测否决，见 `docs/architecture.md` 第 9 节。
