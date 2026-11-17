# ReSlip 索引子系统

`crates/reslip-index` 的模块细节。设计依据见 `docs/architecture.md` 第 5 节；本文件说代码怎么组织、为什么这么切、哪些地方与文档不一致以及原因。实测数字见 `docs/agent-log.md`。

## 模块

```
src/
  lib.rs        // 对外导出
  error.rs      // IndexError：整次操作失败，单个文件读不出不是错误
  probe.rs      // 文件 → AudioMeta，只读文件头
  wav_header.rs // symphonia 拒绝时的 RIFF 头兜底解析
  classify/     // 路径 → 分类（自有词表，见下）
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

词典与打分模型都是本项目自己的产物。`tools/taxonomy.py` 是词表的源文件，
`tools/gen-category-dict.py` 生成 `crates/reslip-index/src/classify/dictionary.rs`。
改词表只改前者，然后重跑生成器与 `cargo test -p reslip-index`。

这套词表是对着真实库写出来的（SoundLibrary 552,155 条路径，迭代用 271,350 条镜像）。
每个词都查过命中量与它横跨的包数，所以 `epic` / `cinematic` / `game` 这类出现在几十个
包标题里的营销词是停用词，而不是把整个库吸进同一个分类的关键词。

**打分方向与直觉相反，原因是实测。** 一条采样库路径长这样：

```
…\Bought\SFX\Epic.Stock.Media.Hybrid.Game.Sounds\…\ESM_SPG_fx_servo_metal_latch_02.wav
```

包目录是营销文案（同样几个词出现在几十个包里），文件名才说这声音是什么。把目录权重给
得比文件名高的模型，打的是营销分：在 271k 真实库上，那种模型仅凭 `epic` 一个词就把
92,876 条（34%）塞进同一个分类。

权重：

| 信号 | 权重 |
| --- | --- |
| 归一化路径里的多词短语 | 4 |
| 文件名里的词典词，每个不同词一次 | 3 |
| 目录名里的词典词，每个目标最多一次 | 1 |

**目录每个目标只投一票**是这里最关键的一条。一个目录里可能有十个词都指向 `Weapons`，
它仍然只算一票：目录说明文件来自哪个包，不说明文件是什么。文件名按词计票，所以
`servo_metal_latch` 这种名字能压过它的包名。

**包名与艺术家名不算描述。** 一个包的每一条文件名通常都以包名开头
（`CHIME_vox_chop_male_ohh`、`Fox_tonal_loop_boops`），那是作者或厂牌，不是声音。
这些词只在文件名首位、且出现在 `BRAND_TOKENS` 里时跳过，不从词典删除：`chime` 同时
是钟声，`ghost` 同时是幽灵，库里两种含义都要。

**只认 ASCII。** 路径里的中文完全不参与分类。Epic Stock Media 那批包的中文目录名是
一整句主题（`【现代化军事战争用户UI界面游戏音效】`），任何一个中文信号都会把那个包里
几千个文件同时贴上同一个标签；声音自己的描述在文件名的 ASCII 部分。

**分词下限是两个字符。** `ui` 是这个库游戏音效包里最常见的索引词之一，三字符下限会
把它直接丢掉。

**短语匹配前先归一化。** `drum_loop` 与 `drum loop` 是同一条短语；匹配原始路径会静默
漏掉库里所有下划线命名。

**同分裁决依赖插入顺序**，用 `Vec` 而不是 `HashMap`，否则遍历顺序随机会让同一份输入每
次分出不同结果。`matched_terms` 全局去重，不是只去相邻重复。

实测（271,350 条真实路径）：未分类 4.7%，23 个分类，133 个 category/subcategory 组合。
分类阶段约占总耗时 17%。

剩下的未分类集中在两类：纯序号文件名（`VEH2 Houseloops - 091.wav`）与只有厂牌/分组词
的文件名（`DPTE White FX - 090.wav`）。这两类本来就没有可分类的词，保持未分类是对的。

`reslip-cli classify-diff` 仍然保留，但它是「旧库与当前词表有多少行不同」的对比工具，
不是一致性验收：词表不同，结果本就不该一致。

## db

表结构与参考实现一致（`sounds` + `sounds_fts` + 三个触发器），新增 `mtime_ns`、`indexed_at` 两列和一个 `meta` 表。保持列名一致是有目的的：旧库能直接打开、原地迁移、逐行比对。

三条容易破坏且难查的不变式：

- **upsert 不碰用户状态。** `favorite` 完全不在 UPDATE 子句里；分类列只在 `category_manual = 0` 时刷新。重扫不能撤销用户的决定。
- **FTS 靠触发器同步，不靠写入方。** 任何 INSERT / UPDATE / DELETE 都会更新 `sounds_fts`，包括 prune 路径。
- **WAL 是必须的。** UI 在索引写入时查询。

**批量装载与逐行维护的取舍。** 三个 FTS 触发器加七个二级索引，逐行维护对单条编辑是对的，对 28 万行的装载是错的：实测写入阶段 16.9s，其中大部分是索引维护。`begin_bulk` / `end_bulk` 在批量装载时卸掉它们，结束时一次重建，写入降到 6–7s。`meta.fts_dirty` 记录中间状态，进程在中间死掉时下次 `open` 会补齐。

**prune 用 `NOT EXISTS` 而不是 `NOT IN (SELECT ...)`。** 后者在 SQLite 上退化成相关扫描，28 万行实测 36s；前者命中主键，3s。

### 用户编辑走批量事务（P4 加的）

收藏、设分类、清分类各有两种形式：单行（`set_favorite` / `set_category` / `clear_category`）与批量（`*_many`）。表上可以 Ctrl+A 选 28 万行再点星标，逐行提交等于 28 万次 WAL fsync；批量形式把整个选区放进一个事务。

**批量写入仍然触发 FTS 触发器，只是每行一次而不是每事务一次。** 这是对的：`category` 与 `subcategory` 在 `sounds_fts` 里，改了分类必须让搜索跟上。批量装载才需要 `begin_bulk` 那种「先卸掉再重建」的做法，用户编辑的量级用不上。

**`summary()` 一次查两个数。** 侧栏同时显示收藏数与总数，分两条 SQL 会在星标点击后的那一帧里让两个数字互相矛盾。`SELECT COALESCE(SUM(favorite), 0), COUNT(*) FROM sounds` 是一趟全表扫描，28 万行实测可接受（它只在编辑后调用，不在滚动路径上）。

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
