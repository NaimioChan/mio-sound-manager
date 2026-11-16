# ReSlip 桌面外壳

`crates/reslip-app` 的模块细节。架构依据见 `docs/architecture.md` 第 4、7 节；本文件说明外壳怎么切、每个决定为什么这么做、以及验收是怎么测的（不是靠人眼看窗口）。

## 模块

```
crates/reslip-app/
  src/main.rs        // 入口，release 下隐藏控制台
  src/lib.rs         // Tauri builder、invoke_handler、窗口、启动时恢复 session
  src/commands.rs    // IPC 线上类型 + 每个命令的薄壳
  src/state.rs       // 引擎、解码 worker、库集合、编辑与 undo
  src/library.rs     // 单个库：索引生命周期、批量写入
  src/session.rs     // 打开过的库与上次活跃的库，写 JSON
  src/history.rs     // undo/redo 栈
  src/error.rs       // AppError → 字符串，跨 IPC 边界
  tauri.conf.json    // 窗口、asset 协议、CSP
  capabilities/      // ACL：core:default + drag + dialog + opener
  tests/ipc.rs       // 不打开窗口就能跑完的验收
ui/                  // SolidJS + Vite，构建产物被编进 exe
  src/App.tsx            // 状态、搜索、选择、快捷键、分页、菜单装配
  src/SoundTable.tsx     // 虚拟列表、多选、键盘、拖拽起点、无限滚动
  src/Sidebar.tsx        // 按库分组的分类 facet / 文件夹树
  src/WaveformDock.tsx   // 波形、播放头、暂停
  src/ContextMenu.tsx    // 右键菜单
  src/CategoryDialog.tsx // 手动分类
  src/SettingsDialog.tsx // 设置页
  src/IndexOverlay.tsx   // 阻塞式索引的读条遮罩
  src/ipc.ts             // 线上类型 + 命令封装
tools/               // run-app / kill-app / CDP 驱动与验收脚本
```

## 三层，各自的规则

**命令层不决定任何事。** `commands.rs` 里的每个 `#[tauri::command]` 只做三件事：校验参数形状、调用 `AppState` 的方法、返回线上类型。逻辑放在 `state.rs`，因为这一层能用 `cargo test` 直接驱动——`tests/ipc.rs` 里的 25 个测试全部走真实的 `AppState`，不经过窗口。只有靠人点按钮才能验证的外壳等于没验证。

**引擎只有一个实例，进程级常驻。** WASAPI 流在 `setup` 里打开，直到进程退出。这跟 P1 的取舍一致：共享模式的 12ms 排队是打开流就付掉的，反复开关只会更慢。设备打不开时应用照常启动，`get_status` 报 `audioAvailable: false` 与引擎自己的错误文本（带设备名或格式），播放命令返回同一条错误——一个坏设备不该让窗口起不来。

**解码在专用 worker 上，队列是"后到者赢"。** 3 分钟 96kHz 文件解码 112ms，跑在命令线程上会把 webview 卡住。`LatestSlot` 是一个只存一件活的信箱：新请求替换排队中的旧请求，被替换的那个收到 `superseded`。按住方向键扫过一个列表时，只解码最终停下的那一行，而不是路过的每一行。

**播放头是纯原子读。** `get_playback_position` 只做几个 `AtomicU64` 载入，不取 `arc-swap` guard、不加锁、不分配——它每秒被调 60 次。`active_frames` 是跟 clip 一起发布的，所以时长也不需要碰槽位。

**编辑走批量事务，并且可撤销。** 收藏、设分类、清分类都是 `*_many` 形式，一个事务写完整个选区。单行版本保留给星标单击，但多选 200 行时逐行提交等于 200 次 WAL fsync。

## 多库（P5）

`AppState` 持有一个 `LibrarySet`：`Vec<Arc<Library>>` 加一个 `active` 根路径。几个决定：

- **一次加锁改整个集合。** 「加入一个库」和「切到它」必须在同一个临界区，否则会出现 `active` 指向一个不在集合里的根这种状态。
- **重开同一个文件夹是「刷新」不是「再开一份」。** 用户在设置里点「重新索引」和点「添加库」选到同一个目录，意图是刷新，侧栏不该出现两条同名项。
- **非活跃库只显示头部。** 它的 facet 计数属于另一个数据库，画在活跃库的位置上会是「属于屏幕上任何东西都不是的数字」。要看它，点一下切过去。
- **切库会清掉过滤条件。** 分类名和文件夹路径都是上一个库的，留着只会得到零行。

## 会话持久化（P5）

`session.rs` 把「打开了哪些库」和「上次活跃的是哪个」写进 `%APPDATA%\com.naimio.reslip\session.json`。

- **存在应用配置目录，不存在库文件夹里。** 索引属于文件夹，而「哪些文件夹开着」属于这台机器上的这个人；把库拷到另一台机器不该带上「它曾在这台机器上打开过」。
- **读失败永不致命。** 文件缺失就是「还没有库」；文件损坏就记一条日志然后当成空的重写。丢掉这个列表的代价是重选一次目录，比一个起不来的应用好得多。
- **文件夹不在了就跳过但保留条目。** 采样库放在移动硬盘上是常态，那块盘启动时可能没接上。它留在 JSON 里（盘可能插回来），但不参与打开。
- **启动时只开数据库，不跑索引。** `attach_session` 做的是 schema 迁移，一分钟的首次扫描留给 UI 去要，那时才有读条可看。

## 阻塞式索引（P5）

`add_library` / `reindex_library` **在索引跑完之前不返回**。这是刻意的取舍：

- 半索引的库能浏览，但看到的是一份缺了兄弟文件的列表，而且缺了哪些你看不出来。
- 一条能说清「还要多久」的读条比这好。
- 280k 文件的首次全量约 64 秒、增量约 5 秒（实测，见 `docs/index.md`）。

实现上的三处细节：

1. **命令跑在 `spawn_blocking` 上，不是命令线程。** `State<'_, AppState>` 借的是命令的栈帧，不能跨进阻塞任务，所以这两个命令拿 `AppHandle` 再从 `app.state()` 取状态。
2. **`onProgress` 是必填的 `Channel`。** Tauri 2 没有 `Option<Channel>`（该类型不实现 `Deserialize`），所以调用方总是传一个；不关心进度的脚本传一个丢弃的通道。
3. **遮罩由 promise 的生命周期驱动，不是另开一个轮询。** 命令只在 pass 结束时 resolve，所以不存在「应用以为自己空闲、数据库其实还在写」的窗口。遮罩铺满窗口、吞掉点击与键盘（`onKeyDown` 在 `indexing()` 时直接 return）。

进度条对两件事是诚实的：遍历完成前 `total` 是 0，此时显示不确定态的横扫而不是从 100% 跳回 3%；最后一阶段（FTS 重建、prune）没有逐文件进度，所以到 100% 时文案换成「finishing up」而不是让条子钉在那里。

## undo（P5）

`history.rs` 保存最近 50 次编辑的**逆操作**：每条记录每个受影响路径的 `(favorite, category, subcategory, category_manual)` 旧值。

- **范围刻意窄。** 只有收藏和手动分类。这是两种最容易误触的编辑（一次误点星标、一次把整个选区归错类），也是采样管理器唯一需要撤销的东西。删文件、改名、重索引不在此列，也不假装支持。
- **一条记录一次撤销。** 200 行的选区是一条历史条目，不是一个 200 深的历史。
- **撤销写回的是精确旧值。** 索引层为此加了 `restore_category_many`：`set_category` 总是置 `category_manual`、`clear_category` 总是重置成 `Uncategorized`，两者都造不出「分类器填的值」这种状态。撤销如果走它们，会把一个猜测静默变成手动选择。这个缺陷是集成测试抓到的。
- **不持久化。** 跨重启的撤销不是一个这种工具该承诺的东西：文件可能已经被重新索引过，过期的逆操作会写回用户已经不想要的分类。
- **库被关掉后，那条记录就作废。** 撤销不能落到另一个数据库上。

## 引擎的四处改动（P1 → P4）

1. **`publish` / `publish_from` / `stop` 改 `&self`。** 它们只写原子量与 `arc-swap`，不需要 `&mut`。改完之后任何线程都能直接发布，命令层不必再搭一个控制线程来串行化。
2. **`publish_from(clip, offset_frames)`。** IPC 的 `start_at` 落到这里。seek 是"在不可变缓冲上换起始位置"，代价与普通播放相同的一次原子写，不重新解码、不裁剪。
3. **`PlaybackMonitor`。** `Engine::monitor()` 克隆一份 `Arc<Shared>` 加格式信息，给 webview 轮询用。`drain_reports()` 也在上面，所以状态栏能显示**实测**的 `publish → 出声`，而不是文档里的目标值。
4. **`pause` / `resume`。** `stop` 清槽并把播放头归零，这是 Stop 该做的；空格需要的是原地冻结。`pause` 只翻一个 `AtomicBool`，渲染回调看到它就写静音、不动 `pos_frames`，clip 留在槽里，所以 `resume` 是一次原子写，不重新解码、不重新 seek，播放头从听众停下的那一帧继续。`publish` 会清掉这个标志——留着它会让新发布的音频静音播放，那是唯一看起来像引擎坏掉的状态。暂停期间也不计下溢：槽里还有 clip，写静音是正确行为。

波形加了 `reslip-engine::waveform`：`Decoder::decode_source` 把文件解成原始采样率与声道数的交错 f32（不做重采样、不做声道映射、不打包字节），`bucket_peaks` 再把它压成 1000 个峰值。

**桶边界是精确的，不是按 256 帧块量化的。** 参考实现先按 256 帧取块最大值再合并到桶，桶边界被量化到 256 帧；在 2 秒文件的 1000 桶视图上，这意味着每个桶的边界最多偏 5.8ms，一个尖锐的瞬态会画到隔壁的柱子上。现在的实现直接按 `[b*frames/buckets, (b+1)*frames/buckets)` 遍历每一帧。

**峰值来自播放用的同一次解码**，包括 gapless 裁剪。否则波形会和声音差 23–25ms——正好是一个瞬态的位置。

## IPC 表面

| 命令 | 返回 | 说明 |
| --- | --- | --- |
| `get_status` | `AppStatus` | 设备、格式、引擎周期、库状态，一次往返 |
| `play(path, startAt?)` | `()` | 发布音频；同一文件重播走缓存 |
| `stop` | `()` | 清空两个槽，播放头归零 |
| `pause` | `()` | 原地冻结，clip 留在槽里 |
| `resume` | `()` | 从冻结的那一帧继续 |
| `get_playback_position` | `PlaybackPosition` | 原子读，给 rAF 轮询；带 `paused` |
| `get_waveform(path, buckets?)` | `WaveformDto` | ~1000 个 f32，附带解码耗时 |
| `add_library(root, full?, onProgress)` | `LibraryStatus` | **阻塞**到索引跑完；进度走 Channel |
| `list_libraries` | `LibraryStatus[]` | 每个打开库的名字、计数、是否活跃 |
| `remove_library(root)` | `LibraryStatus[]` | 关掉一个库，活跃的自动落到下一个 |
| `set_active_library(root)` | `LibraryStatus[]` | 切换显示哪个库 |
| `reindex_library(full?, onProgress)` | `LibraryStatus` | **阻塞**，同上 |
| `search(request)` | `SoundDto[]` | 同 `reslip-index` 的过滤条件，上限 5000 行 |
| `count_matching(request)` | `u64` | 当前过滤命中总数，表尾的 "500 of 27,525" |
| `list_matching_paths(request)` | `string[]` | 全选用的纯路径列表，不受 5000 行上限 |
| `list_folders` | `FolderNodeDto` | 从索引构建的树，不读 `readdir` |
| `get_sidebar` | `SidebarDto` | facet 计数 + 收藏数 + 总数，一次往返 |
| `set_favorite(paths, favorite)` | `u64` | 一个事务，记入 undo |
| `set_category(paths, category, sub?)` | `u64` | 置 `category_manual`，记入 undo |
| `clear_category(paths)` | `u64` | 交还分类器，记入 undo |
| `undo` / `redo` | `string?` | 返回被撤销/重做的编辑名 |
| `get_history` | `HistoryState` | undo/redo 的标签，给按钮文案 |
| `pick_library_folder` | `string?` | 原生目录选择，取消返回 `null` |
| `reveal_in_folder(path)` | `()` | Explorer 里选中该文件 |

线上字段一律 camelCase（`#[serde(rename_all = "camelCase")]`），前端类型手写在 `ui/src/ipc.ts`，与 Rust 侧一一对应。

`search` 与 `list_matching_paths` 分开是刻意的：前者返回完整行且限 5000（一页没有更大的理由），后者只回路径、不设上限。28 万库里 Ctrl+A 要选的是 28 万条路径，不是 5000 条——用 `search` 加个大 limit 会在 5000 处静默截断，而"全选"少选了比没有这个快捷键更糟。

## 前端交互

**选择是锚点 + 集合。** 点击替换、Ctrl+点击切换、Shift+点击从锚点扩展。锚点存的是**路径**不是下标：列表在搜索和排序下会重排，下标会指向另一个声音。

**列表按行窗口虚拟化。** 索引能返回上千行，固定行高让窗口计算是精确的——行高一旦量不准，滚动时就会跳。表头在滚动容器之外，所以它的列轨道要减去滚动条宽度；用 `width: calc(100% - Npx)` 而不是 padding，因为 `box-sizing: border-box` 下 padding 会让 `fr` 轨道按比例吸收差值，两个网格反而偏出两倍滚动条的距离。

**行必须 `left: 0; right: 0`。** 绝对定位的盒子默认收缩到内容宽度，不设左右边界时网格轨道是按文字宽度算的，列会和表头错开。

**无限滚动用 offset 追加。** 请求是排序过的，所以「已加载行数」就是合法的 offset，第二页不会和第一页重叠。重新排序会重置列表，过期 offset 不会活到另一个排序里。触发阈值是距底部两屏——搜索只要 2ms，没有理由等用户撞到底再看空档。

**键盘归列表。** 上下/Home/End/PageUp/PageDown 移动并试听，Ctrl+A 全选当前过滤，空格暂停/继续，Ctrl+Z / Ctrl+Shift+Z 撤销重做，Ctrl+, 开设置。绑定在 window 上但输入框有焦点时跳过，否则在搜索框里打字会逐个字母试听。

**拖拽导出用 `preventDefault` 拦住 DOM drag。** Tauri 的拖拽插件自己发起原生 OLE 拖拽，让 WebView2 同时再起一个 DOM 拖拽会让系统看到两个竞争的拖拽源。插件命令的 `onEvent` 是 `Channel` 且**必填**——不传整条命令直接报 `missing required key onEvent`，拖拽根本不会开始。P0 已实测这个通道从不触发，传它是为了满足签名。

**侧栏宽度可拖。** 把手是 `.rail` 里的绝对定位元素（`right: -3px`），不是 `.body` 的第三个 grid 子元素——那会多出一列并把表格挤走。宽度写进 `localStorage`，范围 160–520px。

**搜索框在列表列顶部，不在窗口顶部。** 它过滤的是列表，所以它属于列表。窗口顶部留给「我在哪个库」和全局动作（undo、设置、自动播放）。

## 验证

**Rust 侧 97 个测试，不需要窗口**（`cargo test --release --workspace`）：

- `tests/ipc.rs` 25 个，跑在真实设备上（本机 FxSound；`RESLIP_TEST_DEVICE` 可换）。P3/P4 部分覆盖：状态报真实设备格式、设备缺失报引擎原文、播放推进播放头、`startAt` 偏移、同文件重播不重新解码、被取代的播放不播旧音频、非音频路径提前拒绝、波形 1000 点且响/静可分辨、桶数夹住、文件夹树来自索引、无库时报错可显示；P5 新增 6 个——两个库同时打开且切换后搜索只看活跃库、关掉活跃库自动落到下一个、切到未打开的库被拒、undo 恢复收藏且 redo 放回、undo 恢复的是**旧分类和旧归属**、多行编辑一次撤销。
- `commands.rs` 7 个：时长/声道标签格式、排序白名单、分页上限钳制。
- `session.rs` 5 个：缺文件是空会话、往返、损坏文件降级、消失的文件夹跳过但保留、活跃库优先。
- `history.rs` 3 个：新编辑清空 redo、栈有上限、标签描述将要发生的事。
- `reslip-index` 44 个、`reslip-engine` 13 个。

**真实 28 万文件库（`D:\reslip-test-library`，本机默认设备 FxSound）：**

| 口径 | 实测 |
| --- | --- |
| `add_library`（增量，返回即完成） | **11.6 s** |
| 搜索 `gunshot` | 38 行，约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| Ctrl+A 全选 | **271,350** 条路径 |
| 无限滚动到 1500 行 | 6 次滚到底，全部追加成功 |

**窗口内交互用 CDP 驱动真实窗口验证，不靠人看：**

- `tools/check-workspace.mjs`（26 项）——加两个库、侧栏分组、切换库、无限滚动、星标、undo/redo（按钮与快捷键两条路径）、设置页三节与即时生效、拖拽改侧栏宽度并持久化、搜索框在列表列、区域拼合无溢出。
- `tools/check-index-overlay.mjs`（7 项）——遮罩立即出现、铺满窗口、光标为 progress、点击打不到列表、空格不会开始播放、结束后消失、列表可用。
- `tools/check-browsing.mjs`（40 项，P4 回归）——分类筛选、方向键、多选、Ctrl+A、右键菜单、星标落库、分类对话框、排序、点击到波形、空格暂停、拖拽命令、文件夹模式。
- `tools/check-layout.mjs`——按真实 DOM 几何断言五个区域的位置尺寸、表头与行列轨道对齐（容差 0.5px）、单元格与表头不被裁切、无元素溢出视口。

## 踩到的坑

**`tauri` 必须开 `custom-protocol`。** 不开的时候 release 构建仍然把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是一片空白窗口——第一次运行正是这样，页面 URL 是 `http://localhost:1420/` 而不是 `http://tauri.localhost/`。开了之后 asset 协议从 exe 内部取文件。

**CSP 必须显式放行 `connect-src`。** P3 的 CSP 只写了 `default-src 'self'`，于是每次 IPC 都会先被 CSP 拦一次再到 `ipc.localhost`——控制台里两条错误，功能靠 Tauri 的兜底路径还能跑。P4 加了 `connect-src 'self' ipc: http://ipc.localhost`。

**`Channel` 不能是 `Option`。** `Option<tauri::ipc::Channel<T>>` 不实现 `Deserialize`，写成可选参数会编译失败。命令签名里它必须必填。

**`State<'_, T>` 不能跨进 `spawn_blocking`。** 它借的是命令的栈帧。要在线程里用状态就拿 `AppHandle` 再 `app.state::<T>()`。

**Windows 不允许替换正在映射的 exe。** `cargo build --release` 会在 `failed to remove file reslip.exe` 处失败。`tools/kill-app.sh` 用 PowerShell 的 `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

**想让 UI 可验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。`playwright-core` 不是本仓库的依赖，它装在参考项目 `slip-sound/` 里；验收脚本用 `createRequire` 指向那个 `package.json`，因此可以从任何目录运行。

**未收藏的星标单元格必须画出字形。** 空 span 宽度为零，点击目标不存在——于是"收藏"只能在已经收藏之后才点得动，等于从行上无法收藏。现在未收藏时画 `☆`，颜色退到边框色。

## 已知未做

- 分类侧栏的折叠状态不持久（展开/收起在会话内有效，重启回到全折叠）；侧栏模式、宽度、设置项都是持久化的。
- 长文件仍整段解码（P1 取舍）；波形因此也是一次整段解码，`decodeMs` 直接显示给用户。
- undo 不跨重启，也不覆盖「删除」「改名」「重索引」。
- 拖拽时悬停的光标样式不变（P0 用户指出过）。
- 不做 Slicer / Effects / Loop / Export，也不做 ASIO 后端——已从路线图移除。
