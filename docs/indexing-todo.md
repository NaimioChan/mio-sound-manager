# 索引建立优化 TODO

## 目标

把索引建立拆成两个用户可感知的阶段：

1. **基础索引**：完成后命令返回，客户端解除遮罩，用户可以正常搜索、筛选、播放和编辑。
2. **metadata 补充**：基础索引完成后在后台继续读取嵌入式 metadata，底部显示非阻塞进度条。

索引建立的扫描、比较和基础探测阶段也必须持续向 GUI 报告状态。用户不能面对一个长时间没有变化的遮罩。

本轮只优化索引建立流程，不改变分类规则、搜索语义、用户编辑语义和音频播放路径。

## 当前基线

基线库：`D:\reslip-test-library`，271,350 个音频文件，主要是 WAV。

最近一次全量索引：

| 阶段 | 耗时 |
| --- | ---: |
| 扫描目录并写入 `_scan_files` | 3.1s |
| 探测 worker | 83.6s |
| 分类累计耗时 | 16.7s |
| SQLite upsert | 17.2s |
| prune | 6.4s |
| FTS、trigram 和二级索引恢复 | 17.4s |
| 总墙钟 | 144.6s |

分类时间是 worker 内每个文件分类耗时的累加值，不是独立墙钟阶段。探测和分类同时发生时，不能把表格中的数字直接相加后当作用户等待时间。

稳定增量索引：

| 阶段 | 耗时 |
| --- | ---: |
| 目录扫描与扫描表写入 | 约 6.5s |
| 增量变化判断 | 约 2.8s |
| 实际探测 | 0 个文件 |
| 实际写入 | 0 行 |
| 总墙钟 | 约 10.7s |

已有旧库的 metadata 一次性回填曾耗时约 261–263s。新流程不能再让这类回填阻塞基础索引和首屏使用。

## 用户可见契约

### 基础索引完成的边界

基础索引完成必须包含：

- 目录遍历完成，或明确报告无法访问的目录；
- 增量模式完成文件变化判断；
- 基础文件探测完成：duration、channels、sample rate、filesize、mtime；
- 基于路径、文件夹和文件名的分类完成；
- 基于文件名的 BPM 完成；
- 基础行已写入 `sounds`；
- 默认列表、筛选、排序和前缀搜索所需的 SQLite 索引已可用；
- full 模式的 prune 已完成；
- 基础 FTS 已可查询。

以下内容不属于基础索引完成条件：

- WAV 的 `bext`、`iXML`、`LIST/INFO`、内嵌 `ID3 `、`_PMX`；
- MP3 ID3v2 description；
- 从嵌入式 metadata 推导的 UCS category/subcategory；
- 仅为 substring fallback 服务的 trigram FTS，是否延后由实测决定。

文件名中的 UCS CatID、路径中的 UCS 词和短语仍属于基础分类输入。它们不需要等待 metadata 阶段。

### 基础索引完成后的行为

- `add_library` / `reindex_library` 在基础索引完成后返回；
- 全窗口遮罩立即关闭；
- 用户可以浏览、搜索、筛选、播放、收藏和编辑；
- metadata worker 继续在后台写入同一个数据库；
- metadata worker 的写入必须保持搜索和 FTS 一致；
- metadata 阶段的失败只影响对应文件，不得让已经完成的基础索引回滚；
- metadata 阶段结束后，底部进度条消失，库状态回到 idle；
- 应用关闭或库被移除时，metadata worker 必须停止并完成连接清理，不得留下半开任务。

## 进度模型

### 后端事件

把当前只有 `{done,total}` 的 `IndexProgress` 升级为带阶段的事件。建议线上结构：

```text
IndexProgress {
  phase: IndexPhase,
  done: number,
  total: number,
  filesSeen: number,
  filesChanged: number,
  filesWritten: number,
  messageKind: optional enum,
}
```

`messageKind` 只传枚举或稳定的数据类型，不能从 Rust 发送英文 UI 文案。前端通过 `t()` 显示文字。

建议阶段：

```text
Scanning          遍历目录，total 未知
Comparing         判断文件是否变化，total 可未知
Indexing          基础探测、分类和写入
Finalizing        prune、基础 FTS 或基础索引恢复
Ready             基础索引完成
Metadata          后台 metadata 补充
MetadataDone      metadata 完成
Failed            当前任务失败
```

不强行把所有阶段压成一个百分比。阶段切换时允许进度条重新开始，避免用错误的权重制造虚假的剩余时间。

### 各阶段进度语义

- `Scanning`：`done` 表示已经发现的音频文件数，`total=0`，显示不确定进度；扫描结束后发一次包含 `filesSeen` 的事件。
- `Comparing`：增量比较期间可以显示不确定进度；如果实现了可靠的候选总数，再显示确定进度。
- `Indexing`：`done` 表示已经完成基础探测、分类和写入的文件数，`total` 表示本次基础索引候选数。full 模式通常是全部文件，incremental 模式是变化文件。
- `Finalizing`：prune、索引恢复和 FTS 操作期间显示不确定进度，不能停在 100% 等待。
- `Ready`：基础命令完成，遮罩关闭。不要让前端等待 `MetadataDone`。
- `Metadata`：底部显示 `done / total`，`total` 是本次需要补充的文件数。
- `Failed`：显示错误通知；如果基础索引已经 Ready，metadata 失败不得重新打开全屏遮罩。

### GUI 行为

基础索引未进入 `Ready` 前：

- 保留现有全窗口遮罩；
- 遮罩中的标题根据 `phase` 变化；
- 扫描和 finalizing 显示不确定进度；
- indexing 阶段显示 `done / total`；
- 窗口仍不可操作，避免用户误以为数据库已经完整可用。

基础索引进入 `Ready` 后：

- 立即移除遮罩；
- 立即刷新当前库的行、facet、文件夹树和 summary；
- metadata worker 运行时，在窗口底部显示一个非阻塞进度条；
- 底部进度条不能拦截列表、波形和菜单操作；
- 进度条只显示当前 active library 的 metadata 任务；后台库的进度在其 `LibraryStatus` 中保存，切换过去时继续显示；
- metadata 完成、失败或被取消后，底部进度条进入对应终态并消失。

进度条的验收以状态和计数为准，不能只看颜色或动画是否在动。

## 实现任务

### A. 建立新的索引任务状态

- [x] 在 `crates/mio-index/src/indexer.rs` 增加阶段事件类型和阶段计数。
- [x] 在 `crates/mio-app/src/library.rs` 把单一 `Progress { running, done, total }` 拆成基础任务与 metadata 任务状态。
- [x] 为每个库保存任务 generation，防止旧 metadata worker 在新的重索引后写回过期结果。
- [x] 每个库最多运行一个基础索引任务和一个 metadata 任务；新的基础索引开始前取消或等待旧 metadata 任务。
- [x] 库被关闭时设置取消标记，worker 在文件边界检查取消状态；不要尝试强行终止正在进行的文件读取。
- [x] `LibraryStatus` 增加 metadata 状态，至少包含 `metadataIndexing`、`metadataIndexed`、`metadataTotal` 和可枚举的错误状态。
- [x] 保留现有 `indexing/indexed/total` 字段的兼容语义，并同步 Rust、IPC、UI 和测试。

### B. 基础探测与 metadata 读取拆分

- [x] 在 `crates/mio-index/src/probe.rs` 拆出基础探测 API：只返回 duration、channels、sample rate、filesize、mtime。
- [x] 将 WAV metadata 读取和 MP3 ID3v2 读取从基础探测默认路径移除。
- [x] 保留 WAV 非标准 `fmt ` chunk 的 fallback，但 fallback 只能服务基础字段，不得顺带扫描完整 metadata。
- [x] 保留 M4A channel fallback；它属于基础字段，不能被 metadata 延后逻辑误删。
- [x] `IndexOptions` 增加明确的 metadata 读取开关，基础索引固定关闭该开关；测试和专用工具可以显式打开。
- [x] `row_from_parts` 和基础分类路径不再依赖 `description` 或嵌入式 UCS metadata。
- [x] 增加 metadata-only API，接收已知的 path、filesize、mtime；worker 复用读标签的文件句柄，在读取前后核对指纹，无需另外按路径打开文件做检查。
- [x] metadata worker 写回时使用 `WHERE path = ? AND mtime_ns = ? AND filesize = ?`，文件在基础索引后发生变化时丢弃本次结果，留给下一次索引。
- [x] filename CatID、filename/path UCS 推断和 BPM 结果必须保持原有结果不变。

### C. 后台 metadata worker

- [x] 基础索引完成后，根据数据库行筛出 WAV/WAVE/MP3 的 metadata 未完成行。
- [x] 只把需要 metadata 的格式加入任务：当前重点是 WAV 和 MP3；其它格式不做无效打开。
- [x] 用独立连接或短时持有 `IndexDb` 锁的方式写入，不能让一个大事务长期阻塞 UI 查询。
- [x] 以批次提交 metadata 更新，默认批次大小为 5,000。
- [x] description 写入必须更新普通 FTS；UCS category/subcategory 写入也必须更新普通 FTS。
- [x] metadata worker 的分类更新遵守 `category_manual`：手动分类不得被覆盖。
- [x] 已手动指定的 UCS 字段通过 `ucs_manual` 所有权标记保护，不被 metadata worker 静默覆盖。
- [x] description 不是用户手动分类状态的一部分，可以在文件内容变化后被新 metadata 替换。
- [x] 单文件读取失败记录失败计数并继续处理后续文件；失败不得让整个 worker 退出。
- [x] worker 完成后写入明确的完成状态；被取消时区分 `cancelled` 和 `failed`。

### D. IPC 与应用生命周期

- [x] 扩展 `crates/mio-app/src/commands.rs` 的 `IndexProgress`，同步 `ui/src/ipc.ts`。
- [x] `add_library` 和 `reindex_library` 的返回点改为基础索引 Ready。
- [x] 基础进度继续通过当前 Channel 发送，扫描、比较、基础索引和 finalizing 都可见；Channel 在 IPC 边界按 100ms 节流，阶段切换立即发送。
- [x] metadata 进度通过 `list_libraries`/`LibraryStatus` 轮询暴露。
- [x] 前端在 metadata 活跃时以约 350ms 频率刷新；metadata 空闲时停止轮询。
- [x] `AppState` 在基础索引返回后立即刷新列表，不等待 metadata worker。
- [x] 后台库也能启动 metadata worker；切换 active library 时不会重新开始任务。
- [x] 同一库重复触发 reindex 返回 busy 错误，禁止两个 writer 无序竞争。
- [x] 应用关闭、移除库和切换库覆盖 worker 的退出、锁释放和 stale generation 场景。

### E. 前端遮罩与底部进度条

- [x] 重写 `ui/src/IndexOverlay.tsx`，按 `phase` 渲染，不再从 `total === 0` 猜测状态。
- [x] `Scanning`、`Comparing`、`Finalizing` 显示不确定进度，`Indexing` 显示确定进度。
- [x] 基础索引收到 `Ready` 或命令 resolve 后关闭遮罩，不等待 metadata。
- [x] 在 `ui/src/App.tsx` 增加 metadata progress signal，并与 active library 绑定。
- [x] 增加底部非阻塞 metadata progress 组件，放在版本号同一行、版本号左侧；不得额外占用一整行。
- [x] 底部组件显示已处理数量和总数；文案进入三语字典。
- [x] metadata worker 运行期间列表仍可滚动、搜索、播放、编辑；CDP 已验证真实行点击、查询、收藏编辑和播放位置读取。
- [x] metadata 完成后底部组件从 DOM 移除，避免空进度条占用状态栏布局。
- [x] metadata 失败显示一次 notice，并保留可用基础索引。
- [x] 多库切换进度隔离；库 A/B 验证 active library 只显示对应 metadata 进度，后台库状态不串库。

### E2. 本轮反馈待办

- [x] 修复阶段事件没有到达 GUI 的问题。CDP 已观察到 `Scanning → Comparing → Indexing → Finalizing → Ready` 的真实切换，遮罩文案与进度条随 phase 更新；Channel 仍在基础命令完成后返回。
- [x] 为阶段事件增加可观测回归：索引器阶段序列测试覆盖空库、0 changed 增量和 full index；IPC 转发器测试覆盖阶段切换立即发送及同阶段节流。
- [x] 增加 description 清洗契约。description 只保留有分类价值的自然语言描述，排除采样率、位深度、声道数、时长、编码格式、容器字段、设备信息、GUID/哈希/十六进制串和其它明显技术参数；不要把整段技术 dump 当成 description。
- [x] description 清洗必须同时作用于 WAV `bext`、iXML、LIST/INFO、ID3 和 `_PMX` 来源；保留 meaningful prose，清洗后为空则写 NULL。已补正例、技术字段反例、十六进制反例及 FTS 清除回归。
- [x] 将 metadata 进度条移动到现有 statusbar 内，与版本号同一水平行，位置在版本号左侧；完成、取消、失败后不留下空白高度。CDP 测得 progress 16px、statusbar 26px，完成后组件移除。
- [x] 修复大数据量列表快速滚动到中段时出现空白的问题。当前 viewport 请求优先、真实 scroll 事件直接调度、可见 segment 受保护，旧请求不会阻塞当前窗口。
- [x] 大列表 CDP 验收至少跳转 10%、30%、50%、80%、95%，每个位置限定时间内出现真实行；连续快速滚动、反向滚动和滚动后排序已通过交互验收。

### E3. 2026-11-25 用户反馈待办

- [x] metadata 进度条只显示一份 `正在读取嵌入式 metadata n/m`。CDP 检查状态栏和组件 Shadow DOM。
- [x] 按描述、UCS 子类升降序连续跳转 10/30/50/80/95% 和反向滚动。27 万行库在后台回填期间逐页核对真实文件路径，本次最慢 305 ms。SQLite 使用排序索引并在分页后计算共享描述计数。
- [x] CDP 在 1280、1024、800、680 px 下验证列名始终在 header 内，含 UCS 子类。
- [x] CDP 验证中文状态栏和设置摘要均无 `引擎周期`。

### E4. 选中条目与底部进度布局

- [x] 在 600、720、900、1280 px 下交替点击短文件名和带长文件名、长分类及长描述的行；表头与行的列宽一致，点击前后不变，播放控件留在窗口内。
- [x] 底部 metadata 轨道与计数在同一水平中心线上；窄窗口下版本号可见，任务结束后状态栏仍为 26 px。

### F. 低风险性能优化

这些项目在基础/metadata 拆分完成后执行。每项都必须在同一真实库、同一磁盘状态下记录前后数据。

- [x] 增量比较一次生成 `_scan_changed`，探测分页和总数共用结果。跳过数由 `files_seen - files_probed` 计算。
- [x] 单列文件遍历、扫描表写入和增量比较时间。
- [x] 对比 `_scan_files` 批量提交大小 5,000、20,000、50,000；结果见下表，保留默认 5,000。
- [x] `end_bulk()` 单列二级索引创建、普通 FTS、trigram FTS 和触发器恢复耗时。
- [ ] 如果 trigram rebuild 占据明显时间，评估延后到后台；普通前缀搜索必须在基础索引完成时可用，substring fallback 在 trigram 尚未完成时必须有明确行为。
- [x] `begin_scan` / `end_scan` 单列计时，扫描表继续随连接释放。
- [ ] 评估 WAV 一次打开同时读取基础头和 metadata 的实现。这个优化只能放在 metadata 已经异步化之后，避免重新把 metadata 引入基础路径。
- [ ] 分类器内部的 `Vec::contains`、临时 `String` 和 `to_string_lossy` 优化放到最后；先用 benchmark 证明它仍是主耗时。

## 数据库与一致性要求

- [ ] 基础索引期间已有 UI 查询继续可用，WAL 保持开启。
- [x] 基础索引提交后，可用文件名进行普通前缀搜索。
- [x] metadata 部分完成时，description 和 UCS 字段允许出现 NULL 与已填值混合状态。
- [x] metadata 更新和普通 FTS 触发器在同一事务提交。
- [x] metadata 补充保留 `favorite`、`category_manual` 和手动 UCS。
- [x] 读取 metadata 前后核对文件指纹，写入时再核对数据库行指纹。
- [x] 关闭桌面窗口后重启，待补充行继续处理，基础索引为 0 changed。
- [x] metadata worker 在同一事务里记录新 `metadata_rule` 并重置受影响格式的待补充标记。
- [ ] `index_version` 在基础行变化和 metadata 行变化时都能让 folder tree、facet 和列表缓存失效。

## 验收测试

### Rust 单元与集成测试

- [x] 基础索引不会调用 WAV description、WAV iXML 或 MP3 ID3 解析。
- [x] 基础索引完成后返回，桌面 IPC/CDP 验证 metadata worker 仍在运行。
- [x] metadata worker 从待补充行继续，已成功的文件保留原值。
- [x] 单文件读取失败后，后续文件的 description 仍写入并可搜索。
- [x] 文件在基础索引和 metadata 之间被修改时结果跳过，下一次重索引后重新读取。
- [x] 手动分类、手动 UCS 和 favorite 由 metadata 回归测试及重索引回归测试覆盖。
- [x] description、UCS category/subcategory 写入后普通 FTS 查询立即可见。
- [ ] worker 取消、库移除、应用 Drop 不会留下正在写入的连接。
- [ ] 增量索引 0 changed 时仍能正确结束，并报告 metadata 无任务或已有后台任务。

### GUI/CDP 验收

- [x] CDP 检查扫描和比较阶段的进度为不确定状态，扫描阶段显示已发现文件数。
- [x] CDP 观察到基础 indexing 阶段的多次计数更新。
- [x] CDP 检查 finalizing 阶段进度为不确定状态。
- [x] 基础索引完成后遮罩消失，列表可搜索、可播放、可编辑。
- [x] metadata 继续时，版本号左侧同一行的进度条出现并前进，且列表和播放控件仍可操作。
- [x] metadata 完成后底部进度条消失，布局不留下空白区域。
- [x] metadata 失败显示 notice，基础列表仍可用；删除 1,500 个文件后失败计数为 1,500，状态进入 failed。
- [x] active library 与 background library 的进度不会串库。
- [ ] 重复点击 reindex 不会启动两个任务，也不会出现两个底部进度条。
- [x] 真实库关闭窗口后重开，metadata 继续提交，基础行数不变。
- [x] 快速滚动大型列表时，10/30/50/80/95% 深度都能渲染真实行；中段不出现长期空白。

### 性能回归

每次改动在同一台机器执行，至少记录：

```bash
cargo run --release -p mio-cli -- index D:\reslip-test-library --basic --full --threads 16 --batch 5000
cargo run --release -p mio-cli -- index D:\reslip-test-library --basic --full --threads 16 --batch 20000
cargo run --release -p mio-cli -- index D:\reslip-test-library --basic --threads 16
cargo run --release -p mio-cli -- probe-bench D:\reslip-test-library --limit 5000 --threads 1
cargo run --release -p mio-cli -- probe-bench D:\reslip-test-library --limit 5000 --threads 16
cargo run --release -p mio-cli -- classify-bench D:\reslip-test-library 50000
cargo test --release --workspace
```

报告至少包含：

- 基础索引墙钟时间；
- metadata 墙钟时间；
- 扫描、比较、探测、分类、写入、prune、FTS、trigram 的独立耗时；
- 基础索引返回时的行数；
- metadata 任务总数、完成数、失败数；
- 进程 RSS 或任务管理器峰值；
- 增量 0 changed 的墙钟时间。

#### 2026-11-25：`D:\reslip-test-library`（271,350 行，16 线程）

| 模式 | 墙钟 | 遍历 | 扫描表写入 | 增量比较 | 探测与分类墙钟 | SQLite 写入 | prune | 二级索引 | 普通 FTS | trigram | 峰值工作集 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 基础全量，写入批次 5,000 | 92.87 s | 2.28 s | 0.65 s | — | 40.71 s | 26.60 s | 5.34 s | 4.44 s | 4.14 s | 7.08 s | 87.4 MiB |
| 基础全量，写入批次 20,000 | 248.42 s | 4.82 s | 0.73 s | — | 47.36 s | 169.50 s | 7.85 s | 3.83 s | 5.34 s | 7.50 s | 123.6 MiB |
| 增量 0 changed，扫描批次 5,000（两次） | 5.25–5.60 s | 2.37–2.48 s | 0.62–0.67 s | 2.05–2.24 s | — | — | — | — | — | — | 50.1–61.6 MiB |
| 增量 0 changed，扫描批次 20,000 | 6.15 s | 2.54 s | 0.75 s | 2.61 s | — | — | — | — | — | — | 61.7 MiB |
| 增量 0 changed，扫描批次 50,000 | 5.33 s | 2.34 s | 0.69 s | 2.10 s | — | — | — | — | — | — | 61.4 MiB |

旧增量路径同轮测得 6.87 s，探测和写入都为 0。全量两次均写入 271,350 行，未 prune；分类的 worker 累计时间为 9.43 s / 13.64 s，已包含在探测与分类墙钟中。扫描表准备不到 1 ms，清理 0.14–0.21 s。触发器恢复不到 1 ms。

扫描批次 50,000 的单次墙钟落在 5,000 的两次测量之间，因此保留 5,000。全量 20,000 的写入在本次测量显著更慢，尚未用重复试验区分批次和磁盘状态的影响。`probe-bench` 实际取样 5,025 个：1 线程 11.32 s，16 线程 0.84 s，顺序执行，文件缓存条件可能不同。`classify-bench` 50,000 路径耗时 1.01 s（49,505 路径/s）。trigram rebuild 约占快档基础全量的 7.6%；仍同步完成，等测清后台重建对 substring 搜索的影响后再决定是否延后。

实际关闭窗口再启动后，metadata 已提交数由 5,000 增至 40,000，再增至 65,000，基础库始终保留 271,350 行。第二次恢复时，CDP 记录基础索引 0 changed、metadata 任务 229,074 个，抽样时已处理 15,000 个、失败 0 个。这次桌面会话峰值工作集 232.3 MiB（仅主进程）。完整回填墙钟与终态失败数待单独测量。

## 推荐执行顺序

1. [x] 完成 Scanning/Comparing/Indexing/Finalizing/Ready 的 CDP 和 Channel 验收。
2. [x] 清洗 description，补技术字段、十六进制反例和 FTS 回归。
3. [x] 把 metadata 进度条放到版本号左侧同一行，完成布局和交互验收。
4. [x] 修复大列表快速滚动中段空白，完成深度跳转、反向滚动和排序后的 CDP 验收。
5. [ ] 继续补数据库一致性、手动编辑、多库切换、关闭恢复和 metadata 失败场景测试。
6. [ ] 完成扫描表、FTS/trigram、batch size 等低风险优化的单项 benchmark。
7. [ ] 最后评估 WAV 一次打开和分类器内部优化。

## 完成判据

本 TODO 完成需要同时满足：

- 基础索引完成后客户端可用，metadata 不再阻塞首轮使用；
- 扫描、比较、基础索引和 finalizing 全程有真实阶段状态；
- metadata 在底部显示进度，且不阻塞用户操作；
- metadata 部分失败不会破坏基础索引和用户编辑；
- 增量 0 changed、full index、旧库 metadata 回填、多库切换和应用关闭均有测试；
- 在真实 271,350 文件库上记录拆分后的阶段耗时，并能解释每个主要阶段的成本。
