# Mio Sound Manager 索引子系统

`crates/mio-index` 的模块细节。设计依据见 `docs/architecture.md` 第 5 节；本文件说代码怎么组织、为什么这么切、哪些地方与文档不一致以及原因。实测数字见 `docs/agent-log.md`。

## 模块

```
src/
  lib.rs        // 对外导出
  error.rs      // IndexError：整次操作失败，单个文件读不出不是错误
  probe.rs      // 文件 → AudioMeta，只读文件头
  wav_header.rs // symphonia 拒绝时的 RIFF 头兜底解析
  bpm.rs        // 文件名 → BPM（只认显式标记）
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

**AIFF/FLAC metadata 不做回填。** 这两种格式在目标库里基本没有可用的描述标签；继续解析只会增加探测成本，不能带来有意义的覆盖率。

**`._` 开头的文件不索引。** macOS 拷出来的 `._name.wav` 是 AppleDouble 资源叉，几百字节的元数据带着音频扩展名。真实库里有 9,455 个，全部探测失败；不过滤的话它们会以「文件损坏」的样子进索引。

**为什么不解码整文件。** 索引阶段解码 28 万个文件是不可接受的（按 P1 的 3 分钟 96k 文件 112ms 外推，是几小时）。头探测够了：duration 从 `n_frames / sample_rate` 得来，两种值都在头里。

## classify

词典与打分模型都是本项目自己的产物。`tools/taxonomy.py` 是词表的源文件，
`tools/gen-category-dict.py` 生成 `crates/mio-index/src/classify/dictionary.rs`。
改词表只改前者，然后重跑生成器与 `cargo test -p mio-index`。

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

`mio-cli classify-diff` 仍然保留，但它是「旧库与当前词表有多少行不同」的对比工具，
不是一致性验收：词表不同，结果本就不该一致。

## classify：完整 UCS A 与 22 类投影

第二套词表来自 [Universal Category System](https://www.universalcategorysystem.com/)
8.2.1（82 个大类、753 个 Category/SubCategory、约一万条同义词）。UCS 是音效馆员的索引，
本项目的 22 个分类是制作人的分类法，两者不是一回事，所以数据库同时保留 native UCS 字段和
22 类投影字段。

**完整 A 的单遍分类在 CatID 层决策。** 文件名的 CatID 是强声明，UCS 词和短语在同一层
参与评分；最终把获胜 CatID 投影成 22 类。手工分类仍由 `category_manual` 保护，不会被重索引覆盖。

```
CatID 声明：8 / UCS 短语：3 / 文件名词词根：2 / 目录每目标：1
```

**CatID 是不猜的。** 文件名以 UCS CatID 开头（`GORESqsh_Slime Squeeze_01.wav`）等于声明了
分类，所以它拿 8 分，压过任何词。匹配**区分大小写**：UCS 里 `RAIN`、`WIND`、`STORM`、
`TEST` 既是 CatID 又是最常见的英文词，折叠大小写会让每个 `rain_on_roof.wav` 都声明成
RAIN。

**同义词要过三道筛，第三道是重点。**

1. **投影。** `tools/ucs_mapping.py` 是手写的 753→我们的映射。`ARCHIVED`（stems、ADR、
   测试音）、`AIR`（压缩空气）、`CHEMICALS` 这三支直接丢弃而不是硬塞：丢在哪一行是看得见
   的，硬塞看不见。
2. **形状。** 一个同义词只有在我们的分类里**恰好命中一个目标**时才保留。`beat`、`hit`、
   `clap` 在 UCS 里横跨好几个分类，在我们的分类里也横跨，这种词放进来只会制造随机结果；
   `misc`、`source`、`designed`、`morph` 是 UCS 自己的桶名或制作术语，不描述声音；
   `one shot` 是文件结构词（43,723 个文件、100 个包），放进短语表会把整个库变成 Music。
3. **实测。** `tools/ucs-evidence.py` 对每个候选词统计两件事：出现在多少文件、多少包里，
   以及在**文件名**含有该词、且本项目分类器已有答案的文件中，答案与 UCS 映射是否一致。
   规则是：一致率 ≥ 0.6 且样本 ≥ 10 才收；样本不足时只有影响面 ≤ 250 个文件才收。被否掉的
   是那些「看起来对、在库里错」的词——`trance`（UCS 里是 MAGIC-SPELL 的同义词，在库里是
   Trance 曲风：46 个文件说 Drums，9 个说 Magic）、`hybrid`（UCS 里是 VEHICLES-CAR 的
   同义词，在库里是包名的一部分：12,454 个文件横跨 19 个包）。每个被排除的词与它的计数都在
   `tools/ucs-evidence.json` 里。

最终收进 6,085 个词、136 条短语、703 个可投影 CatID；native 表仍保留全部 753 条官方
CatID 记录。实测（同一 271,350 条路径）：

| | 未分类 | 分类总数 |
| --- | --- | --- |
| 只有本项目词表 | 12,740（4.70%） | 133 组合 |
| 加 UCS 兜底 | 11,611（4.28%） | 133 组合 |

完整 A 的 Rust/Python mirror parity 以 `mio-cli classify-check` 为硬门槛；CatID 声明和
UCS 词汇会与 curated 词汇共同参与评分。

**维护流程。** UCS 的发布包不进 git（`/docs/UCS Release/` 已忽略，也不随产物分发）。
换新版本时：解压到该目录，依次跑 `tools/ucs-evidence.py`、`tools/ucs-catid-evidence.py`、
`tools/import-ucs.py`、`tools/gen-category-dict.py`。改 `tools/ucs_mapping.py` 的映射也走
同一条链，否则新映射没有证据支撑。

当前数据库已保存官方 native category/subcategory、22 类投影字段和 UCS category/subcategory
的 FTS 搜索。UCS 信息有两条来源：文件名开头的 CatID，以及 WAV iXML
`MusicalCategory` metadata；后者没有 CatID 时仍保存官方 category/subcategory，并投影到
22 类（能映射时）。唯一、证据筛选通过的 UCS 同义词可以写入 native category/subcategory，但不会
凭空生成 `ucs_cat_id`。`ucs_cat_id` 只表示文件名显式声明的 CatID；CLI 的 `index-stats`
同时报告 native category 覆盖和 explicit CatID 覆盖，避免混淆两个口径。

WAV 的 `bext`、`iXML`、尾部 `LIST/INFO`、内嵌 `ID3 ` 与 MP3 ID3v2 的文本描述会进入
`description`，并加入 FTS5；主表也提供 Description 列，支持查看和排序。解析会滤掉技术字段和 APIC 图片。
桌面端完成基础索引后，由后台任务按 `metadata_rule` 补充未完成的行；关闭后再次打开可从待补充行继续。
描述以较低权重参与分类，文件名仍优先。

**`tools/classify-mirror.py` 是测量的脚手架，必须与产品对等。** 每个候选规则都在这个
Python 镜像上跑，因为改一次 Rust 重编太慢。但它会漂移——本轮开发中它就漂过一次（
`UCS_FILE_WEIGHT` 写 1 而 Rust 是 2，`lookup` 让 UCS 词进了 curated 阶段），两次都是
`mio-cli classify-check` 抓出来的。所以规矩是：**先跑 `mio-cli classify-check` 确认 0 差异，
再相信任何镜像给出的数字。**

```bash
python tools/classify-mirror.py --sample 271350 --ucs > expect.json
cargo run --release -p mio-cli -- classify-check expect.json
```

## db

**库文件叫 `mio-sm.db`，不再是 `slip-sound.db`。** 列名保持与参考实现一致是有目的的（旧库能直接打开、原地迁移、逐行比对），但文件名是我们自己的东西。`IndexDb::open` 会先做一次改名：`slip-sound.db` 存在而 `mio-sm.db` 不存在时，把主文件连同 `-wal` / `-shm` 一起改过来。**journal 必须跟着改**——SQLite 按名字找 WAL，主文件改名而日志留在旧名下，等于让引擎认为上次是干净关闭的，最后一个已提交事务就丢了。两个名字都在时（迁移中途被打断）保留新文件、不动旧文件，因为猜哪个是当前版本比留一个陈旧文件更糟。

表结构与参考实现一致（`sounds` + `sounds_fts` + 三个触发器），新增 `mtime_ns`、`indexed_at`、
`bpm`、`description`、`ucs_category`、`ucs_subcategory`、`ucs_cat_id`、`tags`、`cover_hash`、
`details_indexed` 十列，`sound_details` / `covers` 两张表和 `meta` 表。schema 版本 5。

**详情三列与两张表（schema 5）。**

- `sounds.tags` 是清洗后、可搜索的标签文本，进 `sounds_fts`；`sounds.cover_hash` 是图片字节的 32 位
  hex；`sounds.details_indexed` 标记详情 pass 是否处理过这一行。它**不复用** `metadata_indexed`：
  重跑标签提取不该把 28 万行重新分类一遍。
- `sound_details(sound_id, tags_json, tags_truncated, updated_at)` 存面板要的原始标签列表
  （`[{key, value, source, searchable}]`）。`details_indexed=0` 时这一行**不得展示**——文件被重写后，
  旧标签属于旧版本的文件。
- `covers(hash, thumb, thumb_mime, source_path, source_mtime_ns, updated_at)` 按图片哈希存一张 48px
  缩略图；同一张专辑封面出现在 400 个文件里只解码一次、存一份。

**upsert 对新增字段沿用指纹规则。** `(mtime, size)` 未变就保留 `cover_hash` / `tags` /
`details_indexed`；变了就把三者清空、标记 0，由下一次 details pass 重新填。旧的 `sound_details` 行
留着不删（每行一次 delete 不值，而且 `details_indexed=0` 本身就让它不可见）。

**FTS 加 `tags` 列，`fts_version` 3 → 4。** 这是结构性变更：列集变了就必须 drop / recreate /
rebuild，否则查询用的列和索引描述的列不是同一个东西，升级前写入的标签会静默搜不到。
`sounds_trigram` 不变（只搜文件名）。

**`idx_cover_hash` 与 `idx_sample_rate`。** 面板的"同封面"和采样率过滤都是等值条件，没有索引就是
点击背后的一次 289k 行扫描。排序靠 `idx_cover_hash` + `NULLS LAST`，`EXPLAIN QUERY PLAN` 在单测里
断言（`cover_filter_and_sort_use_the_index_and_group_by_hash`、
`numeric_filters_are_exact_and_combine_with_other_filters`），不凭感觉加复合索引。

三条容易破坏且难查的不变式：

- **upsert 不碰用户状态。** `favorite` 完全不在 UPDATE 子句里；分类列只在 `category_manual = 0` 时刷新。重扫不能撤销用户的决定。
- **FTS 的常规维护靠触发器，批量装载靠重建。** 逐行路径上，`sounds` 的 INSERT / UPDATE / DELETE 都由触发器同步到 `sounds_fts`，prune 也走同一条路。批量路径是例外：`begin_bulk` 明确 `DROP TRIGGER`，`prune_scan_missing` 又跑在 `end_bulk` 之前，所以装载期间 `sounds_fts` 不实时；正确性由 `end_bulk` 的最后一次整表 rebuild 兜底（`meta.fts_dirty` 记录这个中间态）。别把"触发器覆盖一切"当成理由去在批量路径里相信 FTS 的中间结果。
- **WAL 是必须的。** UI 在索引写入时查询。

**`ucs_cat_id` 单独建索引。** 它是 UCS 侧栏的第三级筛选（`s.ucs_cat_id IN (…)`），也是分类对话框里每个 CatID 计数的 `GROUP BY` 键。没有索引时两者都是全表扫描：参考库上 CatID 过滤一页 5.9 s、计数 447 ms，而计数就挂在星标点击的刷新路径上。索引之后是 0.4 ms 与 0.1 ms。它在 `migrate()` 里建一次，并成对出现在 `begin_bulk` 的 DROP 与 `end_bulk` 的 CREATE 列表里——漏了任何一边，批量装载后它就没了或者装载时白维护。

**批量装载与逐行维护的取舍。** 三个 FTS 触发器加七个二级索引，逐行维护对单条编辑是对的，对 28 万行的装载是错的：实测写入阶段 16.9s，其中大部分是索引维护。`begin_bulk` / `end_bulk` 在批量装载时卸掉它们，结束时一次重建，写入降到 6–7s。`meta.fts_dirty` 记录中间状态，进程在中间死掉时下次 `open` 会补齐。

**`bpm` 来自名字与路径，升级时回填。** 取值分四层，先命中先赢：文件名里的 `bpm` 标记（`128BPM`、`174 bpm`、`BPM120`，以及 `126-5 BPM` / `BPM_138.4` 这两种一位小数写法）→ 文件名里的 Splice 约定 `(90, Em)`（括号里必须是一个 tempo 加一个**调名**，`(69, Atonal)` / `(69, C to Fm)` 不算）→ 祖先文件夹名里的同样标记，就近取，**命名区间的段跳过**（`83-98 BPM` 说的是这个包覆盖的范围，不是其中某个文件的速度）→ 循环类文件名里的裸数字。

前三层是文件自己声明的，第四层是推断，所以它有两个闸门。**名字必须自称循环**（`loop(s)` / `beat(s)` / `groove(s)` / `break(s)` / `topline` / `melody` / `riff` / `arp` / `chord(s)` / `bassline` / `stem(s)`，含 `drumloop`、`houseloops` 这类复合词）：`Snare_120.wav`、`Kick_808.wav`、`NIG065.wav` 里的数字是编号或包号，而下面那条小节检验会欣然把 `NIG065.wav` 读成 65 BPM 共 33 小节。**长度必须整小节**：剪切过的循环首尾落在小节线上，容差 25ms。这条判据是量出来的——在 21,812 行 tempo 由显式标记确知的数据上，真 tempo 命中小节线的比例是 90%，随机一个 60–200 的数只有 3%。第四层的裸数字区间收窄到 60–200（低于 60 的多是小节数，例如某个包把 32 小节的循环叫 `Loop 32`）。同一个数字在名字里出现两次（`DS_DSP_150_synth_loop_pad_rage_150_Dm`）算一个候选，不因重复而放弃。

**小节检验只否决循环，不否决一次性采样。** 循环是按小节线切的，长度才是证据；一次性采样是一个音头加尾巴，它的长度不承载任何音乐信息。把这条检验套到全体文件上，在 28 万行的库里有 174 行被清空，而按文件自己的标记判断，其中 74% 是错的——被误杀的主力是 `100BPM - Lullaby` 这类文件夹里的 42 秒人声长句（17.5 小节，天生不落在小节线上）。所以文件夹层只在名字自称循环时才用时长否决它。

**绝不解码音频来推断速度**，头里没有 tempo，从波形估一个既要解码每个文件又会在半速/双速上系统性出错。

`bpm` 列是 v1→v2 迁移加的，而值是**从没变过的文件名、目录名和时长**推出来的：增量索引按 `(mtime, size)` 跳过未变文件，所以重新索引永远不会重算它们。升级路径因此不靠重新索引，而是 `open` 时拿已存的 `filename` / `folder` / `duration` 回填一遍。`meta.bpm_rule` 记的不是「做过没有」而是**规则版本号**：更宽的规则必须能区分于更窄的那一版，否则旧库里当初被规则放过的 NULL 行会永远空着。回填期间 FTS 触发器卸掉：`bpm` 不在 FTS 的可搜索列里，逐行触发只是白跑 28 万次分词。28 万行的库实测首次回填约 26s，之后每次 open 约 1s。

**prune 用 `NOT EXISTS` 而不是 `NOT IN (SELECT ...)`。** 后者在 SQLite 上退化成相关扫描，28 万行实测 36s；前者命中主键，3s。

### 用户编辑走批量事务（P4 加的）

收藏、设分类、清分类各有两种形式：单行（`set_favorite` / `set_category` / `clear_category`）与批量（`*_many`）。表上可以 Ctrl+A 选 28 万行再点星标，逐行提交等于 28 万次 WAL fsync；批量形式把整个选区放进一个事务。

**批量写入仍然触发 FTS 触发器，只是每行一次而不是每事务一次。** 这是对的：`category` 与 `subcategory` 在 `sounds_fts` 里，改了分类必须让搜索跟上。批量装载才需要 `begin_bulk` 那种「先卸掉再重建」的做法，用户编辑的量级用不上。

**`summary()` 一次查两个数。** 侧栏同时显示收藏数与总数，分两条 SQL 会在星标点击后的那一帧里让两个数字互相矛盾。`SELECT COALESCE(SUM(favorite), 0), COUNT(*) FROM sounds` 是一趟全表扫描，28 万行实测可接受（它只在编辑后调用，不在滚动路径上）。

收藏那次编辑只需要这两个数，不需要 facet 树，所以 IPC 上另有一条 `get_summary`：`get_sidebar` 的两个 `GROUP BY` 是星标不可能改变的，付一次就是白扫两遍全表。

**批量 setter 的 `UPDATE` 带差异谓词。** `set_favorite_many` / `set_category_many` / `clear_category_many` / `set_ucs_category_many` 的 `WHERE` 除了主键还带着「这个字段确实要变」的条件。SQLite 的 `changes()` 数的是匹配行而不是变化行，没有谓词时重复点同一个星标会报「改了 1 行」，于是记一条逆操作是空操作的 undo 条目、白跑一遍 FTS 触发器，界面也会重查一次什么都没变的列表。

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

**基础索引复用遍历阶段的文件信息。** Windows 上 `DirEntry::metadata()` 直接取自 `FindNextFileW` 的结果。之前 `walkdir`、`probe_file`、增量检查各 stat 一次，实测 8.2s 浪费在重复 stat 上。后台 metadata 读取发生在基础索引之后，需在打开的文件句柄上核对 size 和 mtime，避免将已变化文件的描述写到旧行。

**增量索引是稳态的主要路径。** `(mtime)` 匹配就跳过探测。28 万文件第二次运行：5.4s，探测 0 个文件。

## details（标签与封面）

`crates/mio-index/src/details.rs` 只做两件事：抽原始标签、给封面算身份。**不解码音频，不解码图片**——
缩略图是 app 的事（`crates/mio-app/src/covers.rs`），index crate 没有理由依赖一个图片编解码器。

**`searchable` 是一个承诺。** 标了 searchable 的值必须出现在 `sounds.tags` 里（也就是 FTS 里）；
被清洗拒绝的值标成不可搜索，面板显示成普通文本。搜索文本只从这一份列表推导（`search_text()`），
所以两者不可能不一致。长 GUID、超过 4KB 的 dump、几乎不可打印的值都归入"只显示、不搜索"。

**复用 WAV 的 RIFF walker 和 ID3 reader，不写第二套。** `probe.rs` 的 `read_metadata_from` 与新加的
`read_details_from` 共用同一个 chunk 遍历和 `read_id3v2` 调用：两套解析器意味着 `data` 块越过 EOF
这种已经修过一次的情况要修第二次。

**各格式的来源：**

- WAV：`bext`（固定偏移的描述/录制者/日期/时间/时间参考 + 编码历史）、`iXML`（`ATTR` 块与常见裸标签）、
  `LIST/INFO`（已知四字码 + 未知码原样保留）、内嵌 `ID3 ` 块、`_PMX`。
- MP3 / FLAC / OGG / M4A：symphonia 的元数据读取器，遍历**所有 revision**——OGG 的 picture 是独立的
  revision，只读 `current()` 会漏掉封面。
- AIFF：可选内嵌 `ID3 ` 块；其余格式自有 chunk 不做第二套解析，文件照样标记 details done。

**封面选择：** 优先 `FrontCover`，否则第一张可用的图；`-->`（URL）、空数据、超过 32MB 的跳过。
哈希是 `sha256(b"mio-cover\0" + 图片字节)` 的前 16 字节——哈希的是图片，不是标签容器，所以同一张
JPEG 嵌在 MP3 与 FLAC 里能匹配，重新编码的不会（已确认接受）。

**sidecar 解析按目录缓存。** 候选顺序是 `cover.*` / `folder.*` / `front.*` / `album.*` / `artwork.*` /
`art.*`，其次"该目录唯一的图片"，再往上找祖先目录。**一个目录里有几张互不相关的图片时不猜**——
猜错会把同一张图贴到不相干的文件上，而哈希会声称它们是"同一张封面"。candidate 按路径排序，所以一个
worker 里只保留最近一个目录的解析结果就够。**sidecar 的变化不会被音频 mtime 发现**，只有全量重建索引
或重新跑 details pass 才会刷新。

**details pass 和 metadata pass 是同一个 worker，两个分支。** 候选查询是
`metadata_indexed = 0 OR details_indexed = 0`，一次覆盖两个半场；`details_rule` 是独立于
`metadata_rule` 的版本键，改标签提取器不会重分类 28 万行（`prepare_details_pass` 只重置
`details_indexed` / `tags` / `cover_hash`）。

**缩略图只为库里还没有的 hash 解码。** 一批 candidate 先收集 hash 去重，再查 `covers` 一次，已知的
直接跳过。失败（打不开、图片坏）按"无缩略图"处理并标记该行 done，不让一个坏文件每轮都回来。

**全量 pass 结束后才清孤儿缩略图。** `DELETE FROM covers WHERE hash NOT IN (...)` 只在 `sounds` 已知
完整时正确——每批清会删掉一行还没重写到的封面。

**测量（本机，真实 289k 库的副本；原库只读，写入全部落在副本上）：**

| 口径 | 实测 |
| --- | --- |
| schema 4 → 5 迁移 + 全量 details pass | **288,955 行，0 失败，0 deferred，455,844 ms（7 分 36 秒）** |
| 有封面的行 / 不同封面 | 73,032 行（25.3%）/ 191 张 |
| `covers` 表体积 | 191 行共 329,881 B，平均 1,727 B/张，最大 5,899 B |
| 数据库增长 | 712 MB → 1,020 MB（+307 MB）。其中 `sound_details.tags_json` 208 MB、`sounds.tags` 63 MB；封面缩略图只占 0.3 MB |
| 一屏 33 个封面 key 的批量缩略图 | p50 **9.8 ms**、p95 **18.6 ms**，返回 33/33，payload 69 KB |
| `inspect_sound` 冷读 | p50 **18.5 ms**、p95 **21.0 ms** |
| `inspect_sound` LRU 命中 | p50 **8.9 ms**、p95 **10.2 ms** |
| 封面等值过滤 / 封面排序 | p50 6.0 ms / 23.7 ms（排序返回全表 288,955 行计数） |
| FTS tags 列（`Vintage`） | 1,274 行，p50 15.2 ms |
| `check-scroll-deep`（真实库） | 5 次深度跳转 134–217 ms 全部绘出真实行 |

19 文件小库上一次 details pass 是 54 ms（19/19）。**库越大，每行越贵**：7 分 36 秒 / 288,955 ≈
1.6 ms/行，而小库是 2.8 ms/行——差距来自真实库的 WAV 需要走 RIFF 尾部 chunk，小库的 chunk 更短。

数据库增长的大头不是封面而是标签文本（208 + 63 = 271 MB）。若要把 `sound_details` 的 JSON 压小，
第一步是只存面板真正会显示的 key，而不是全部可搜索值——那是另一个决定，本轮没做。

