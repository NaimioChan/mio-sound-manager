# ReSlip 索引子系统

`crates/reslip-index` 的模块细节。设计依据见 `docs/architecture.md` 第 5 节；本文件说代码怎么组织、为什么这么切、哪些地方与文档不一致以及原因。实测数字见 `docs/agent-log.md`。

## 模块

```
src/
  lib.rs        // 对外导出
  error.rs      // IndexError：整次操作失败，单个文件读不出不是错误
  probe.rs      // 文件 → AudioMeta，只读文件头
  wav_header.rs // symphonia 拒绝时的 RIFF 头兜底解析
  classify/     // 路径 → 分类（移植自 Slip Sound）
    mod.rs
    dictionary.rs  // 生成物，勿手改
  db.rs         // schema、upsert、prune、搜索、facets、文件夹树
  folders.rs    // 文件夹范围查询与树
  indexer.rs    // 遍历 → 探测 → 分类 → 写入，分阶段计时
```

契约是 `SoundRow`：一个文件在某一时刻的全部可索引事实，加上它属于哪个相对文件夹。写入侧只认这一份结构。

## probe

`probe_file` 读容器头，不解码。三个字段（duration / channels / sample_rate）加 size 与 mtime，后者是增量索引的判断依据。

**为什么不用自己写解析器。** symphonia 的 probe 在 0.09–0.45ms 内给出全部字段，自己写七种格式的解析器只会引入偏差。两个例外各有一段兜底，原因都是实测撞到的：

1. **WAV 的 `fmt ` chunk 长度为 20 字节时被拒**。symphonia 的 `wave/chunks.rs::read_pcm_fmt` 只接受 16 / 18 / 40，其它长度直接 `malformed fmt_pcm chunk`。20 字节是合法的 `WAVEFORMATEX`（`cbSize = 2`），真实库里 28 万文件中有 284 个是这种，全部是可正常播放的 PCM，ffprobe 读得出。兜底解析器在 `wav_header.rs`，只读 RIFF 头，不碰音频数据。
2. **M4A 头里没有声道数**。symphonia 的 `isomp4` 把 `codec_params.channels` 留空，值在 sample description 里。解码首包才能拿到，成本约 0.3ms，只对需要它的文件做。

**`._` 开头的文件不索引。** macOS 拷出来的 `._name.wav` 是 AppleDouble 资源叉，几百字节的元数据带着音频扩展名。真实库里有 9,455 个，全部探测失败；不过滤的话它们会以「文件损坏」的样子进索引。

**为什么不解码整文件。** 索引阶段解码 28 万个文件是不可接受的（按 P1 的 3 分钟 96k 文件 112ms 外推，是几小时）。头探测够了：duration 从 `n_frames / sample_rate` 得来，两种值都在头里。

## classify

参考实现的移植。词典由 `tools/gen-category-dict.mjs` 从 `slip-sound/src/main/category-dictionary.ts` 生成，不手抄——463 个关键词手抄必错。

**两处必须保持参考实现的语义，否则分类结果会变：**

- **同分裁决依赖插入顺序**。参考遍历 `Object.entries(scores)`，是插入顺序。用 `HashMap` 遍历顺序随机，实测让真实库 271,350 行中的 3,932 行归到了不同分类。现在用 `Vec<(String, u32)>` 保持插入顺序。
- **`matched_terms` 是全局去重**（`[...new Set()]`），不是只去相邻重复。`Vec::dedup` 只合并相邻项，短语和单词同时命中时会留下重复。

一致性不是断言，是测量出来的：`reslip-cli classify-diff <folder> <reference.db>` 用 Rust 分类器重新分类旧库里的每条路径，逐字段比对。当前结果：271,350 行，category / subcategory / confidence / matched_terms 四项零差异。

**非 ASCII 字符被丢弃而不是转写。** 这是参考行为，保持它才能让一致性比对有意义：`Kemençe` 贡献的 token 是 `kemen`，和以前一样。

## db

表结构与参考实现一致（`sounds` + `sounds_fts` + 三个触发器），新增 `mtime_ns`、`indexed_at` 两列和一个 `meta` 表。保持列名一致是有目的的：旧库能直接打开、原地迁移、逐行比对。

三条容易破坏且难查的不变式：

- **upsert 不碰用户状态。** `favorite` 完全不在 UPDATE 子句里；分类列只在 `category_manual = 0` 时刷新。重扫不能撤销用户的决定。
- **FTS 靠触发器同步，不靠写入方。** 任何 INSERT / UPDATE / DELETE 都会更新 `sounds_fts`，包括 prune 路径。
- **WAL 是必须的。** UI 在索引写入时查询。

**批量装载与逐行维护的取舍。** 三个 FTS 触发器加七个二级索引，逐行维护对单条编辑是对的，对 28 万行的装载是错的：实测写入阶段 16.9s，其中大部分是索引维护。`begin_bulk` / `end_bulk` 在批量装载时卸掉它们，结束时一次重建，写入降到 6–7s。`meta.fts_dirty` 记录中间状态，进程在中间死掉时下次 `open` 会补齐。

**prune 用 `NOT EXISTS` 而不是 `NOT IN (SELECT ...)`。** 后者在 SQLite 上退化成相关扫描，28 万行实测 36s；前者命中主键，3s。

## folders

两条来自参考实现、值得原样保留的做法：

- **文件夹过滤是字符串区间，不是 `LIKE`。** `path >= base\ AND path < base]` 走主键索引，且不会被 `Foley` 过滤器误收 `Foley2`。`LIKE 'Foley\%'` 需要转义且无法用索引。
- **树从索引里的路径构建，不读 `readdir`。** 索引已经知道每个文件；再问一次文件系统会显示空文件夹和索引没访问过的文件夹。

**所有路径必须归一化到原生分隔符。** `walkdir` 返回的路径继承传入根的分隔符，根传 `D:/lib` 会得到 `D:/lib\sub\a.wav` 这种混合形式。这不只是难看：它破坏了唯一键，让一次全量扫描把上一轮写的行全部 prune 掉（实测 271,350 行全删）。`normalize_root` / `store_path` 在入口处统一，`folder_range` 与 `folder_tree` 也要对根做同样处理，否则文件夹树会是空的。

## indexer

```
walk  →  probe（并行）  →  classify（同一 worker）  →  write（批量事务）  →  prune  →  FTS 重建
```

六个阶段分别计时。分开的原因是不同的阶段以不同的方式失败：遍历慢说明文件太多，探测慢说明线程不够或磁盘慢，写入慢说明事务形状不对。只报总数会掩盖是哪一个。

**并行只在探测阶段。** 遍历是单次目录扫描（IO 密集且已经很快），SQLite 写入天然串行。探测是每毫秒 8 次独立头读取，这是 8 线程能买到 3.2 倍的地方。

**线程数默认取逻辑核数，不设上限。** 架构文档的 8 线程上限来自 NVMe 上的合成库。真实 28 万库在无 DRAM 的 SSD 上，8 → 16 线程还有 22% 收益（4,347 → 5,331 文件/s）：探测是随机读延迟受限，不是 CPU 受限，更多在途请求让盘能重排。没有实测不要往下调。

**`stat` 只做一次。** Windows 上 `DirEntry::metadata()` 直接取自 `FindNextFileW` 的结果，不额外系统调用。之前 `walkdir`、`probe_file`、增量检查各 stat 一次，实测 8.2s 浪费在重复 stat 上。

**增量索引是稳态的主要路径。** `(mtime)` 匹配就跳过探测。28 万文件第二次运行：5.4s，探测 0 个文件。
