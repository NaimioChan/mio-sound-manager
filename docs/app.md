# Mio Sound Manager 桌面外壳

`crates/mio-app` 的模块细节。架构依据见 `docs/architecture.md` 第 4、7 节；本文件说明外壳怎么切、每个决定为什么这么做、以及验收是怎么测的（不是靠人眼看窗口）。

## 模块

```
crates/mio-app/
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
ui/                  // SolidJS + Vite + @siemens/ix，构建产物被编进 exe
  src/main.tsx           // 注册 iX 自定义元素、应用主题、挂载
  src/App.tsx            // 状态、搜索、选择、快捷键、分页、菜单装配
  src/SoundTable.tsx     // 虚拟列表、多选、键盘、拖拽起点、无限滚动
  src/Sidebar.tsx        // 按库分组的分类 facet / 文件夹树
  src/WaveformDock.tsx   // 波形、播放头、框选区、循环与暂停、输出音量、手势图例
  src/Inspector.tsx      // 右侧详情面板：文件信息、分类/UCS、原始标签、封面
  src/ContextMenu.tsx    // 右键菜单（视口坐标，非 iX dropdown）
  src/CategoryDialog.tsx // 手动分类（iX modal）
  src/SettingsDialog.tsx // 设置页（iX modal，含主题选择）
  src/IndexOverlay.tsx   // 阻塞式索引的遮罩
  src/theme.ts           // 配色主题系统：调色板 → iX token 覆盖
  src/icons.ts           // 图标 SVG（仓库内 src/assets/icons/），转 data URL 给 ix-icon
  src/i18n.ts            // 语言信号 + t()；locale 存 mio:locale
  src/locales/{en,zh,ja}.ts // 三语字典，en 是键集基准
  src/format.ts          // 表格/面板共用的字节、采样率、BPM 格式化
  src/ix-elements.d.ts   // iX 自定义元素的 Solid JSX 类型
  src/styles.css         // 布局与几何，颜色一律取自 iX token
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
- **重开同一个文件夹是「刷新」不是「再开一份」。** 右键一个库点「Reindex」和点「Add library」选到同一个目录，意图是刷新，侧栏不该出现两条同名项。
- **非活跃库只显示头部。** 它的 facet 计数属于另一个数据库，画在活跃库的位置上会是「属于屏幕上任何东西都不是的数字」。要看它，点一下切过去。
- **每个库自己的管理动作在它自己的右键菜单里。** 切库、Reindex、Full reindex、Reveal、Close 都在库头部的右键菜单，不在设置里——设置只能作用于「当前那个库」，而这里有多个。
- **切库会清掉过滤条件。** 分类名和文件夹路径都是上一个库的，留着只会得到零行。

## 会话持久化（P5）

`session.rs` 把「打开了哪些库」和「上次活跃的是哪个」写进 `%APPDATA%\com.naimio.miosound\session.json`。

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

卡片里只有标题、进度条、`done / total` 三行，下面那句「此过程完成前窗口保持锁定」删掉了：它不改变用户在任何时刻的能做的事（窗口本来就锁着，进度条就在回答「还要多久」），属于标题的注解。

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

波形加了 `mio-engine::waveform`：`Decoder::decode_source` 把文件解成原始采样率与声道数的交错 f32（不做重采样、不做声道映射、不打包字节），`bucket_peaks` 再把它压成 1000 个峰值。

**桶边界是精确的，不是按 256 帧块量化的。** 参考实现先按 256 帧取块最大值再合并到桶，桶边界被量化到 256 帧；在 2 秒文件的 1000 桶视图上，这意味着每个桶的边界最多偏 5.8ms，一个尖锐的瞬态会画到隔壁的柱子上。现在的实现直接按 `[b*frames/buckets, (b+1)*frames/buckets)` 遍历每一帧。

**峰值来自播放用的同一次解码**，包括 gapless 裁剪。否则波形会和声音差 23–25ms——正好是一个瞬态的位置。

## IPC 表面

| 命令 | 返回 | 说明 |
| --- | --- | --- |
| `get_status` | `AppStatus` | 后端、设备、格式、引擎周期、库状态，一次往返 |
| `list_audio_devices` | `AudioDeviceDto[]` | WASAPI 端点与 ASIO 驱动合成一份列表，给设置里的设备选择 |
| `asio_compiled` | `bool` | 这个二进制是否带 ASIO 支持 |
| `set_audio_device(backend, device?, asioBufferFrames?)` | `AppStatus` | 切设备；打不开不报错，返回的状态里带原因 |
| `play(path, startAt?)` | `()` | 发布音频；同一文件重播走缓存 |
| `stop` | `()` | 清空两个槽，播放头归零 |
| `pause` | `()` | 原地冻结，clip 留在槽里 |
| `resume` | `()` | 从冻结的那一帧继续 |
| `get_playback_position` | `PlaybackPosition` | 原子读，给 rAF 轮询；带 `paused` |
| `set_gain(gain)` | `f32` | 输出增益，`1.0` 为单位增益；返回引擎实际持有的（夹过的）值 |
| `get_gain` | `f32` | 引擎当前增益，给音量滑杆回读 |
| `set_loop(enabled, startS?, endS?)` | `LoopState` | 循环开关与区间；`startS`/`endS` 缺省或倒置表示整个文件。只写原子，不重新 publish |
| `get_loop` | `LoopState` | 引擎当前的循环状态，给开关回读（设备切换后仍准确） |
| `export_region(path, startS, endS)` | `string` | 把选区按源采样率与声道写成 32-bit float WAV 到缓存目录，返回路径。给「把框选拖出去」用 |
| `get_waveform(path, buckets?)` | `WaveformDto` | ~1000 个 f32，附带解码耗时 |
| `add_library(root, full?, onProgress)` | `LibraryStatus` | **阻塞**到索引跑完；进度走 Channel |
| `list_libraries` | `LibraryStatus[]` | 每个打开库的名字、计数、是否活跃 |
| `remove_library(root)` | `LibraryStatus[]` | 关掉一个库，活跃的自动落到下一个 |
| `set_active_library(root)` | `LibraryStatus[]` | 切换显示哪个库 |
| `reindex_library(root?, full?, onProgress)` | `LibraryStatus` | **阻塞**，同上；`root` 缺省是活跃库 |
| `search(request)` | `SoundDto[]` | 同 `mio-index` 的过滤条件，支持任意 offset 的窗口查询，单次最多 5000 行。`categories` / `subcategories` 是数组：分类之间 OR，分类与子分类之间 AND |
| `count_matching(request)` | `u64` | 当前过滤命中总数，决定虚拟列表总长度 |
| `list_folders` | `FolderNodeDto` | 从索引构建的树，不读 `readdir` |
| `get_sidebar` | `SidebarDto` | facet 计数 + 收藏数 + 总数，一次往返 |
| `get_summary` | `SidebarSummaryDto` | 只要收藏数与总数，不做 facet 的 `GROUP BY`；星标刷新走这条 |
| `set_favorite(paths, favorite)` | `u64` | 一个事务，记入 undo |
| `set_category(paths, category, sub?)` | `u64` | 置 `category_manual`，记入 undo |
| `clear_category(paths)` | `u64` | 交还分类器，记入 undo |
| `set_favorite_matching(request, favorite)` | `u64` | 按 filter 批量收藏/取消收藏，可附排除路径，记入 undo |
| `set_category_matching(request, category, sub?)` | `u64` | 按 filter 批量设置分类，可附排除路径，记入 undo |
| `clear_category_matching(request)` | `u64` | 按 filter 批量清除手动分类，可附排除路径，记入 undo |
| `undo` / `redo` | `string?` | 返回被撤销/重做的编辑名 |
| `get_history` | `HistoryState` | undo/redo 的标签，给按钮文案 |
| `get_library` | `LibraryStatus?` | 活跃库的状态；无库时 `null`。`get_status` 里也带一份，这条给只关心库的调用方 |
| `pick_library_folder` | `string?` | 原生目录选择，取消返回 `null` |
| `reveal_in_folder(path)` | `()` | Explorer 里选中该文件 |
| `inspect_sound(path)` | `SoundDetailsDto` | 详情面板的一行：原始标签（按来源分组，带 `searchable`）、封面身份与 512px data URL、`lazy` |
| `get_cover_thumbnails(keys)` | `CoverThumbDto[]` | 一批封面身份对应的 48px 缩略图，只查 `covers` 表；最多 128 个，找不到的不返回 |

线上字段一律 camelCase（`#[serde(rename_all = "camelCase")]`），前端类型手写在 `ui/src/ipc.ts`，与 Rust 侧一一对应。

Ctrl+A 不向 renderer 返回路径列表。前端以 filter 快照 + 排除集表示选择；批量编辑命令在 Rust 侧解析目标。`search` 仍按有限窗口返回行，避免深列表遍历时累积整库 DTO。

## 前端交互

**选择是锚点 + 集合。** 点击替换、Ctrl+点击切换、Shift+点击从锚点扩展。锚点存的是**路径**不是下标：列表在搜索和排序下会重排，下标会指向另一个声音。

**列表按需窗口化。** 滚动高度始终按匹配总数计算；固定行高让任意 index 可直接定位。窗口以稀疏段缓存读取，最多保留 24 段；缺失段显示骨架并按需请求。表头在滚动容器之外，所以它的列轨道要减去滚动条宽度；用 `width: calc(100% - Npx)` 而不是 padding，因为 `box-sizing: border-box` 下 padding 会让 `fr` 轨道按比例吸收差值，两个网格反而偏出两倍滚动条的距离。

**行必须 `left: 0; right: 0`。** 绝对定位的盒子默认收缩到内容宽度，不设左右边界时网格轨道是按文字宽度算的，列会和表头错开。

**无限滚动用 offset 追加。** 请求是排序过的，所以「已加载行数」就是合法的 offset，第二页不会和第一页重叠。重新排序会重置列表，过期 offset 不会活到另一个排序里。触发阈值是距底部两屏——搜索只要 2ms，没有理由等用户撞到底再看空档。

**键盘归列表。** 上下/Home/End/PageUp/PageDown 移动并试听，Ctrl+A 全选当前过滤，空格暂停/继续，Ctrl+Z / Ctrl+Shift+Z 撤销重做，Ctrl+, 开设置。绑定在 window 上但输入框有焦点时跳过，否则在搜索框里打字会逐个字母试听。

**拖拽导出用 `preventDefault` 拦住 DOM drag。** Tauri 的拖拽插件自己发起原生 OLE 拖拽，让 WebView2 同时再起一个 DOM 拖拽会让系统看到两个竞争的拖拽源。插件命令的 `onEvent` 是 `Channel` 且**必填**——不传整条命令直接报 `missing required key onEvent`，拖拽根本不会开始。P0 已实测这个通道从不触发，传它是为了满足签名。

**`draggable` 必须绑布尔值，写成裸属性会让整条拖拽链路静默失效。** Solid 把 `draggable`（无值）渲染成 `draggable=""`，而这个属性是枚举型（`true`/`false`/`auto`），空串不是合法值，浏览器回落到 `auto`，于是 `row.draggable === false`、DOM 拖拽永远不发起、`onDragStart` 从不执行。整条「拖进 DAW」的产品功能因此完全没反应，而代码看起来是对的。正确写法是 `draggable={true}`，检查脚本断言 `row.draggable === true` 与 `getAttribute('draggable') === 'true'`。

**侧栏宽度可拖。** 把手是 `.rail` 里的绝对定位元素（`right: -3px`），不是 `.body` 的第三个 grid 子元素——那会多出一列并把表格挤走。宽度写进 `localStorage`，范围 180–520px。

**搜索框在列表列顶部，不在窗口顶部。** 它过滤的是列表，所以它属于列表。窗口顶部留给「我在哪个库」、undo/redo 和设置（图标）。播放的三个控件在波形 dock 的右端聚成一组：`AUTO PLAY` 是文字按钮，暂停/继续与停止是图标——前者的状态是「点一行会不会响」这条策略，12px 的图标说不出来。

**顶栏左侧是产品标记加产品名。** 标记是 `ui/src/assets/brand.svg`，`App.tsx` 用 `import brandLogo from './assets/brand.svg'` 交给 Vite——它会带上 hash 进 `dist/assets/`，被 CSP 的 `img-src 'self'` 覆盖，也不会在组件里再抄一份路径。用 `<img alt="" aria-hidden="true">`：产品名就在旁边，读屏再念一遍标记是噪音。高度 26px、宽度自适应（原图 1000×829），`flex: none` 让名字在窄窗口下压不到它。**标记不跟着主题换色**：那两组蓝是品牌色，不是界面色，`--theme-*` 换成 Tokyo Night 的紫时 logo 保持原样才对。

**库条目在视觉上高于它下面的树。** 库头部是 650 字重、带底色块、顶部一条分隔线，活跃库的背景是 `ghost--selected` 并带左侧 2px 强调条（与选中分类行、选中文件行同一组 token）；树行是 400 字重、无底色。这不是装饰：库条目带自己的右键菜单，看起来像树的一行会让人想不到那里能右键。库头**没有折叠三角**——库不是树节点，把正在看的库折起来正好是设计拒绝的状态；行首留一个 `.twisty spacer` 只为保持名字缩进。折叠整条侧栏是另一个功能，会有自己的入口。

**设置页五节：Audio / Library / Columns / Behaviour / Appearance。** Audio 是输出设备选择（WASAPI 端点与 ASIO 驱动合成一个下拉）加一行当前格式与周期。Library 是当前库、显示索引、添加库、每页行数。Columns 是逐列的显隐复选框（两列排布，收藏星禁用）。Behaviour 是选中即播放与无限滚动。Appearance 是主题卡片、界面缩放与语言。库的管理动作（切库、重建索引、关闭）不在这里——见「多库」。

**设置项不解释自己。** 一行就是标签加控件；唯一的第二行文字是控件说不出来的状态：设备打不开的原因、当前实际打开的格式与周期、某个动作会作用到多少条。主题卡片只保留名字与色板，原来那六句「适合长时间使用」之类的说明删掉了，`Theme` 上也不再带 `hintKey`。产品名下面那句 `samples`、设置弹窗标题下的 `Device, library, appearance`、分类弹窗的 `N sounds` 同样删掉了；`N sounds` 改到正文第一行，因为它说的是这次操作的范围，不是标题的注解。

**界面缩放走 webview 的 zoom factor，不是 CSS。** `zoom.ts` 调 `getCurrentWebview().setZoom(0.8…1.5)`，WebView2 的 `SetZoomFactor` 把布局、文字、图片一起重排，命中区域和波形画布都跟着变；换成 CSS `zoom` 或 `rem` 倍率会让画布留在设备像素上、表格那套 px 几何也留在原处。该能力不在 `core:default` 里，`capabilities/default.json` 必须显式加 `core:webview:allow-set-webview-zoom`，否则命令被 ACL 拒绝而界面上只表现为「选了没反应」。存储键 `mio:zoom`，取值就是百分比，选择器显示什么就存什么。

**排序名和行字段名是两套。** 渲染层按行字段排序（`durationS`、`sampleRate`），索引层白名单用的是 SQL 列名（`duration`、`sample_rate`）。`SORT_WIRE`（`ui/src/columns.ts`）是唯一做这个翻译的地方。第一版把行字段名直接当 `sortBy` 发出去，`search` 报 `unknown sort column durationS`；而且因为失败那次之后排序值没变，后续每次搜索都带着同一个坏名字，于是点一次 Length 之后分类和文件夹筛选也一起失效——直到用户改去点 Name。

**列宽可拖、列可显隐、宽表横向滚动。** 列定义、顺序、显隐与宽度只有一个拥有者：`ui/src/columns.ts`（`SortKey`、`COLUMNS`、`SORT_WIRE`、`DEFAULT_DIR` 都在那里，`SoundTable` 不再导出）。**新 profile 默认显示 Cover / Name / Category / Length / Ch / BPM / Rate / Size**：UCS 两列与 Description 仍可从表头菜单和设置打开，但它们占的是采样浏览最常读的列宽；`mio:column-hidden` 里已有数组（包括空数组）时以用户自己的排列为准。表头、骨架行、真实行三处用同一个 `gridTemplate()`，并且**全部是像素轨道**——`fr` 会随窗口重算，用户拖出来的宽度换一个窗口尺寸就变成另一个值。代价是窗口收窄或界面缩放时会出现横向滚动条，这是有意的取舍（Explorer 同理），不要为了消灭滚动条把 `fr` 加回来。宽度存 `mio:column-widths`、显隐存 `mio:column-hidden`、顺序存 `mio:column-order`，各自容错、各自整体替换（原地改 `Set` / 对象不会触发 Solid 通知）。表头每列右侧有 5px 的 `.head-resize` 把手：拖动时窗口级监听、`pointercancel` 也收尾、`pointerup` 时落盘；拖宽期间必须把表头的 `draggable` 关掉（把手是排序按钮的子元素，不关就会把拖宽变成拖列序），松手后还要吞掉一次 click（否则顺手排了个序）。双击把手恢复默认宽。隐藏排序列时回退到 Name 排序，否则列表按一个看不见的列排序、箭头也消失。设置页的 Columns 一节与表头右键菜单都能切换显隐；`favorite` / `filename` 不可隐藏、不可拖、不可缩放。

**横向滚动时表头被 `translateX` 平移跟随，不做 `sticky`。** 表头在滚动容器之外，`sticky` 会让它占掉 28px 滚动内容高度，于是 `viewportSegments`、`scrollToRow`、spacer 高度全部要改；包装层（`.table-head-wrap` 裁切 + 表头 `translateX(-scrollLeft)`）只多一个平移量。表头宽 `max(totalWidth, 100% - scrollbar)`，行的占位 div 用同一个 `min-width`，所以溢出与否两边右边界都对得上——`check-layout` 会在 `scrollLeft=120` 时再断言一次，因为不滚动的表头在偏移 0 处看不出问题。

**第一页查询在进行中时 `data-querying="true"`。** 列表不阻塞：旧行还在、还能点，但查询落地时要整批换掉。这个标志既是表尾的 `searching…`，也是 `tools/` 里 CDP 脚本等待的信号——之前脚本靠 sleep 猜，猜早了点在即将被卸载的行上，选中跟着丢，看起来像点击处理器坏了。

**文件夹树按需挂载，展开状态按库持久化。** 子节点只在展开时进 DOM：真实库有 27 万个文件，全量渲染一遍再靠 CSS 隐藏，每次侧栏重绘都要建几千个节点。展开集合存在 `localStorage` 的 `mio:folders-expanded:<root>` 里，切库时跟着换——不存的话每次重绘都折叠回根，深目录根本点不进去。`.twisty` 是可点的 12px 方块（3px 宽的三角点不中），行本身点击是选筛选，双击是展开/折叠。

**分类与子分类可多选。** 点击替换、Ctrl+点击增删、再点唯一已选中的那行则清空。两侧的筛选语义不同：分类之间是 `IN (…)`（或），分类与子分类之间是 AND。这也是为什么点一个子分类会同时点亮它的父分类——只亮子行而不亮父行，两个筛选条件相交就是空集，界面上却看不出原因。行上带 `data-category` / `data-subcategory` / `data-folder`，选择器不依赖翻译过的文案。

**侧栏模式是一个下拉，不是一个分段控件。** Categories / UCS / Folders 三选一，控件是 `ix-select`：三个标签在 232px 的侧栏里排成一行会挤到看不清，而设置页已经用同一套下拉做同类选择。模式只由 `App.tsx` 持有（`mio:sidebar-mode`，旧键 `mio:facet-mode` 做一次性迁移），因为切换模式同时要清空分类侧的五个筛选数组——同一个事实有两个拥有者就会互相打架。切模式**不清** `folder` 和 `favoritesOnly`：它们是独立筛选，各自有 chip，用户没点就不该消失。选完模式后焦点要还给窗口：`ix-select` 的内部输入框会留住焦点，而 `keyboard.ts` 把 `IX-SELECT` 判作文本输入，不 blur 的话方向键在下拉里移光标而不是移列表。

**文件夹行有自己的右键菜单。** 面板与行菜单、库菜单是同一个 `ContextMenu`；条目是「在文件管理器中显示」「只看此文件夹」「复制文件夹路径」。根行（All folders）没有菜单——它代表整个库，三个动作都无从谈起。

**右键菜单里不再有「拖进 DAW」。** 拖拽是鼠标动作，菜单条目只能写一句说明、点不动；把一条禁用的说明放在动作中间，读起来像功能坏了。拖拽本身仍在（行 `draggable` → `startDrag`）。

**BPM 是排序列，不是 tag。** 值只从名字和路径取（`crates/mio-index/src/bpm.rs`），四层按优先级：文件名里的 `bpm` 标记 → 文件名里的 Splice 约定 `(90, Em)` → 祖先文件夹名里的标记（区间段跳过）→ 循环类文件名里的裸数字。裸数字这一层要两个条件同时成立才认：名字自称循环，且**时长是整小节**（容差 25ms，真 tempo 命中率 90%，随机数 3%）。所以 `Snare_120.wav` 这类一次性采样仍然是空值——编号和速度猜错一个都会自信地参与排序。时长只用来否决循环，不用来否决一次性采样：采样是一个音头加尾巴，长度不承载音乐信息，拿小节线卡它会把 42 秒的人声长句误杀。显式标记的范围是 20–300，裸数字收窄到 60–200，`126-5 BPM` 和 `BPM_138.4` 这种写法按一位小数读。空值在正反两个方向都排在最后（`ORDER BY s.bpm IS NULL`），和 duration 同一个约定。BPM 不参与侧栏筛选：它连续、取值多，做成 tag 只会多出一排没人点的行。

**单行星标通常不重新查询。** 没开收藏过滤、没按 ★ 排序时，星标只改行上的布尔值。旧路径每次都推进 `dataEpoch`，丢掉所有缓存窗口并重取可见段，同时重算侧栏统计。在 288,955 行的库上，单独测量旧版 CatID 分组查询用了 447 ms。

现在按编辑种类分流（`editThenRefresh(apply, kind, …)`）：

- 单行或按明确路径收藏，且视图不按星标过滤或排序：保留缓存和滚动位置。写入成功后，`SoundTable` 合并对应路径的 `favoriteOverride`，侧栏只读 `get_summary`（收藏数和总数），撤销按钮读 `get_history`。
- 「只看收藏」、按 ★ 排序，或者按筛选条件批量收藏：重查列表，避免行的位置或未显示的缓存窗口保留旧星标。侧栏仍只读 `get_summary`。
- `setCategory` / `clearCategory`：刷新 facets 与 summary，不读 UCS 目录；`setUcsCategory` 才读，因为只有它写 `ucs_cat_id`。
- 编辑返回 0 行（重复点同一个星标、给已经同分类的行再设一次分类）直接返回，不刷新、也不进 undo 栈。批量 setter 因此带 `AND (差异谓词)`：SQLite 的 `changes()` 数的是匹配行，不是变化行。

`list_ucs_categories` 与编辑、撤销、重做都在阻塞工作线程上执行。界面按操作顺序提交写入，后端串行执行编辑及其历史记录。`sounds(ucs_cat_id)` 的索引会在打开库时补建，并在批量装载结束时重建。同一个库上的 SQL 探针中，CatID 计数从 447 ms 降到 0.1 ms。

**波形上的手势由落点决定，不用修饰键。** 三个区域：顶部三分之一是「拖出去」（悬停时高亮并显示 grab 光标）、选区边界 ±5px 是「改边界」（col-resize 光标）、其余是「单击定位、拖拽框选」。原先拖出去要按 Alt，那样每次指针离开窗口都得重新按住修饰键，而拖拽本身是一次连续动作；改成空间分区后，面板自己用高亮带说明哪里是把手。

框选的判定靠位移而不是事件类型：`pointerdown` 记下锚点，位移小于 3px 就在 `pointerup` 时当作单击 seek，超过就成为框选。用 `pointerdown` 而不是 `click` 是让框选跟着指针走——选区在按住时就长出来，而单击仍然精确落在原来的那一帧上。选区有 10ms 下限：3 分钟文件上 2px 的拖拽只选出几毫秒，既不构成可用循环也不值得写出成 WAV，所以夹住而不是拒绝。拖边界时另一条边固定当锚点，所以改出来的范围不会整体漂移。

**拖出去带什么，看手势起点。** 起点落在选区内就拖选区（渲染成 WAV），落在选区外或没有选区就拖源文件。这条规则不需要额外解释，也不需要用户记住两个手势：把手在面板顶部，想拖整个文件就在空白处按下。

**框选是「压暗外面」，不是「染亮里面」。** 早期版本在选区上叠 16% 的 accent，静音段看着还行，一遇到满幅波形就消失——波形柱本来就是 accent 同色系，叠同色系上去等于没叠。现在改成：选区外铺一层背景色的半透明遮罩（`--mio-region-scrim`，主题推导，深色主题变暗、浅色主题变亮），选区里只加 10% accent 提亮，再加两条 accent 描边（`--mio-region-edge`，比 accent 与底色各往对比方向混 35%）。判据与波形亮度无关：外面总是比里面暗（或亮），边缘总是 2 个设备像素。

**波形是纯色柱，不是条纹。** 之前每根柱宽 `barW - 0.5` 且坐标落在小数像素上，125% 缩放下每根柱跨两个设备列各半覆盖，抗锯齿把整块画成明暗交替的竖条纹。现在整层画布都在设备像素里算、不开 `setTransform`，每根柱的左右边界各自 `Math.round`，相邻柱正好拼满、一色到底。

**波形按声道分层。** 单声道一条、双声道上下两条，各带一条零线；超过两条声道退回单条合并包络（理由见 `docs/engine.md` 的 waveform 一节）。分道之后左右声道不对称的文件（硬声像、单边静音）才看得出来，这是 DAW 里判断素材时的第一条信息。

**循环是引擎状态，不是渲染层状态。** `set_loop(enabled, region)` 只写三个原子（`loop_start`、`loop_end`、`looping`，最后写 `looping`，这样半写的范围不会被回调看见），渲染回调在 `Mixer::fill` 里分段拷贝并在循环点回绕，不需要重新 publish——重新加载 clip 会让每次改选区都咔一声。范围按 clip 帧数夹住，空或倒置的范围回落到整个文件；播放头在范围外时从范围起点进入。`looping` 为真时 `finished` 永不置位，这正是「循环」的意思。设备切换后状态会重建，所以 `AppState` 在引擎外另存一份镜像（和 gain 同一个理由），重开引擎时重新灌进去。

**框选区域拖到外部要先落成文件。** DAW 接受的是文件投放，不是帧范围，所以拖选区走 `export_region`：把选区按**源文件自己的采样率与声道数**写成 32-bit float WAV，落在 app 缓存目录（`%LOCALAPPDATA%\com.naimio.miosound\exports`），再把那个路径交给 `startDrag`。写进库目录是错的——用户没要求保留的文件不该出现在他自己的索引里。拖整个文件不需要渲染，源文件本身就是 DAW 能读的文件。

**鼠标手势写在波形下方一行。** 框选、拖出去、拖边界在面板上完全看不出来，所以 `.dock-hints` 用 `<kbd>` 列出六条：拖拽框选、单击定位、顶部拖出、拖边界、Esc 清除、L 循环。这一行是控件图例，不是说明文档，所以只有一行、10px、`--theme-color-soft-text`，文案全部走 `t()`。

**循环按钮必须是 `ix-icon-toggle-button`，不是 `ix-icon-button`。** iX 5 的 `ix-icon-button` 在 `render()` 里把 `selected` 硬写成 `false`，所以传 `selected={props.looping}` 什么也没发生——循环开着和关着在像素上完全一样，这就是「开关指示不明确」的真正原因。`ix-icon-toggle-button` 把 `pressed` 转成同一个 `.selected` 类，并且补上 `aria-pressed`。它的 `secondary` 按下态是 accent 的 16%，在深色 dock 上仍然太淡，所以 `#transport-loop.on` 用 `--theme-btn-tertiary--*` 一组 token 把它覆盖成实心 accent + 对比色文字，并且覆盖 hover/active/pressed 全状态，避免按下的一瞬间闪回淡色。开关之外，标题与 `aria-label` 会说出当前循环的是什么（`整个文件` 或时间区间），因为「开」本身不区分 400ms 选区与三分钟整曲。

**AUTO PLAY 的开关态用同一套解法：`#auto-play.on` 覆盖 `--theme-btn-secondary--*`。** 它的 `secondary` 按下态是 `--theme-color-component-7`（accent 16%），未按下态是 accent 14%，两者在 dock 的 `--theme-color-2` 上几乎同色——开和关在像素上分不出来，正是循环按钮当初的问题。开态是实心 `--theme-color-primary` + `--theme-color-primary--contrast` 文字与边框；关态是透明底 + `--theme-color-4` 描边 + soft 文字，`hover` 时升到 3 号边与普通文字。hover/active/pressed/pressed-hover/pressed-active 与 disabled 全部覆盖，缺一个就在某个交互态闪回 16%。**这个类必须用 `classList={{ on }}` 写**：`class={…}` 会被 Solid 整体赋给 `className`，把 Stencil 的 `hydrated` 抹掉，iX 于是把按钮隐藏——表现是「点一下就消失」。`check-themes.mjs` 对七套主题各测一次 primary/contrast 对比度（≥4.5），`check-interactions.mjs` 断言默认值为开并在结束时还原 profile，`check-tags.mjs` 连点三次断言它仍 hydrated、仍有渲染尺寸。

**静音是临时静音，不是把音量改成 0。** 取消静音必须回到用户调的那个值，所以「回到哪个值」存在独立的 `mio:unmute-gain`，只在滑杆离开 0 或按下静音时写入。这里有个真 bug：`Number(localStorage.getItem(k)) || 1` 会把存着的 `0` 当成「没存」——静音后重载显示 100%，下一次取消静音又把这个 100% 覆盖掉记忆值。`readStoredGain` 只对缺失或非数值回落。

**类名不要用 `.empty`。** 它是整窗空状态（`padding: 40px`、`height: 100%`），任何 `.foo.empty` 都会连带命中。叶子节点那个占位 twisty 因此叫 `.twisty.spacer`。

## 西门子 iX 设计系统

前端用 `@siemens/ix` 5.3 的 web components 重写，布局与交互不变。组件层面用到的：`ix-button`、`ix-checkbox`、`ix-icon-button`、`ix-toggle-button`、`ix-slider`、`ix-input`、`ix-toggle`、`ix-select`、`ix-chip`、`ix-pill`、`ix-modal`、`ix-progress-indicator`、`ix-icon`。表格、侧栏、右键菜单、波形 dock 仍是自己的 DOM——iX 没有虚拟化表格，而 27 万行的列表不能交给一个每行都建组件的实现。

**颜色一律来自 `--theme-*`。** `styles.css` 里没有一个字面色值（只有几何尺寸）。这是主题系统能工作的前提：主题改一次 token，iX 组件、自有 CSS、canvas 三处一起变。

**Solid 与 iX 的接线有五处要注意：**

1. **普通属性绑定就够。** `value={signal()}` 会写成 property，signal 变化时 iX 组件跟着更新（浏览器里验过，不需要 `prop:` 前缀），但 `data-*` 不是——Solid 把它当 property 赋值，自定义元素没有那个 property，属性就不会出现在 DOM 里。要断言用的键写在包装元素上，别写在 iX host 上。
2. **CustomEvent 用 `on:` 前缀。** `on:valueChange`、`on:checkedChange`、`on:pressedChange`、`on:closeChip`。Solid 走 `addEventListener`，`detail` 在 `event.detail` 上。
3. **`aria-label` 会被 iX 搬走。** iX 的 `InheritAriaAttributesMixin` 把 host 上的 `aria-*` 移到 shadow root 内的 `<button>` 上并**从 host 删除**。所以 CDP 脚本读标签要读内层按钮，读 host 会拿到 `null`。
4. **类名用 `classList`，绝不用 `class`。** Solid 的 `class={…}` 是 `className` 的整体赋值，会把 Stencil 的 `hydrated` / `sc-*` 一起覆盖掉；iX 对 `:not(.hydrated)` 的 host 是 `visibility:hidden`，症状是控件「用一下就消失」。
5. **`vite.config.ts` 的 `IX_TAGS` 是这个项目的 iX 白名单。** 构建后 `trimIxChunks` 把不在名单里的 entry 换成 `data:text/javascript,`，CSP 拒绝加载，标签永不升级：设置页那块区域就是一片空白，控制台只有 `Constructor for "ix-x#undefined" was not found`。新增任何 `<ix-*>` 都要同步那里，并先在 `ix-elements.d.ts` 补类型。

**iX 元素在 Solid 里需要类型声明。** `src/ix-elements.d.ts` 给用到的每个标签声明了属性；基础类型继承 `JSX.DOMAttributes<HTMLElement>`，否则连 `onClick` 都不认。`children` 也要显式声明——Solid 的 JSX 不自动加。

**modal 是命令式打开的。** 渲染出 `ix-modal` 之后调 `showModal()`（内部 `dialog.showModal()` 加动画）。host 在 `.visible` 之前是 `display: none`，而内层 `dialog` 是 `position: fixed`，所以**量 host 的 `getBoundingClientRect()` 永远是 0×0**，检查要量 shadow root 里的 `dialog`。

**iX 会自己关掉对话框，所以状态要跟事件走。** 标题栏的 × 和 Escape 都不经过我们自己的 `close()`：iX 的 `modal-header` 调 `parentDialog.dismissModal()`，`dialog` 的 `cancel` 也走同一条路。只在自己的按钮里清信号，结果是 × 关掉之后父组件仍以为对话框开着——`settingsOpen` 一直是 `true`，下次点齿轮是把同一个值赋给同一个信号，什么都不渲染，看起来就是「设置再也打不开，只能重启」。`SettingsDialog` 与 `CategoryDialog` 都监听 `on:dialogClose` / `on:dialogDismiss` 并调 `props.onClose()`，状态因此跟着实际关闭走。

**图标必须是 data URL 或本地文件，不能用名字。** `ix-icon` 拿到名字时不是查注册表，而是拼出 `svg/<name>.svg` 去 fetch——那个目录不在打包产物里，于是每个图标都 fetch 失败、回退成 iX 的错误符号（一个带叉的方块）。iX 自己的组件为什么没事：它们从 `@siemens/ix-icons/icons` 导入 data URL。

图标现在放在 `ui/src/assets/icons/`（从 `@siemens/ix-icons/svg/` 复制，MIT），`icons.ts` 以 `?raw` 导入并拼成 `data:image/svg+xml;utf8,<原始 SVG>`。两个细节缺一不可：

- **不要 `encodeURIComponent`。** iX 内部只调 `decodeURI`，它不还原 `%3D` 这类保留字符。编码过的 SVG 会被解析成属性字面量为 `width%3d"512px"` 的 `<svg>`，每条 path 丢掉 `d`，渲染结果是一个空框——看起来像"图标没加载"，实际是"图标解析坏了"。
- **去掉 `<?xml …?>` 声明。** iX 用 `DOMParser` 按 `text/html` 解析，XML 声明在那里是 bogus comment；一半的图标文件开头就有它。

放仓库里而不是依赖包里，是为了离线可用：运行时零网络请求，也不依赖包的内部资源布局。

**`ix-select` 的 host 是 `inline-block`，会收缩到最宽选项的宽度。** 设置页里它因此比同一行的按钮短一截、右边空出几十像素。修法是在行内把它拉满：`.setting ix-select { display: block; width: 100% }`（`ix-input` 同理）。

**`ix-dropdown` 不用于右键菜单。** 它锚定元素，而右键菜单要开在光标处、针对一行可能下一帧就被卸载的行。菜单是自己写的面板，颜色取 `--theme-menu--*` 与 `--theme-color-ghost--*`，看起来仍是这套设计系统。

## 多语言

`ui/src/i18n.ts` 加 `ui/src/locales/{en,zh,ja}.ts`，三语：英文、简体中文、日文。

**翻的是外壳文案，不是数据。** 分类名（`Weapons`、`Foley`）、文件名、采样率都是数据：分类名被分类器写进数据库，界面翻译它会让 facet 筛选发一个库里没有的字符串。唯一的例外是 `Uncategorized`——它是 schema 的默认值而不是分类结果，所以显示时翻译（`categoryName()`），比较时仍用原串。

**键集由类型保证。** 英文是 `as const` 的基准，另两语是 `Record<MessageKey, string>`，漏键或拼错键是编译错误。`tools/check-i18n.mjs` 补类型管不到的一条：某语种丢掉了英文里的 `{placeholder}`（渲染成"3 个"而不是"3 个声音"）或者凭空多一个（屏幕上出现字面量 `{count}`）。

**后端不再发英文文案。** undo/redo 原本跨 IPC 传 `"Add to Favorites"` 这种字符串，界面照着显示——那条路径无论怎么翻都翻不到。改成传 `EditKind` 枚举（`favorite` / `unfavorite` / `setCategory` / `clearCategory`），词在前端。`SoundDto.channel_label` 同理删掉，只留 `channels: Option<u32>`，由 `channelLabel()` 出词。

**`<html lang>` 跟着 locale 走。** 它决定 Windows 给 CJK 文本挑哪套字体、文本框拿到哪个输入法；留在 `en` 会让中文用错字形。

**语言选项各写自己的语言。** 选择器里是 `English` / `简体中文` / `日本語`——给看不懂英文的人显示 "Chinese" 的选择器是唯一不该被翻译的菜单。

## 配色主题系统

`ui/src/theme.ts`。一个主题由**八个颜色**描述（背景、前景中性色、两个强调色、对比色、success/warning/alarm/info/neutral），其余两百多个 token 由它们推导：中性 ramp 从背景混到前景，ghost 与 component 填充是前景色带 alpha，选中态是强调色带 alpha，语义色的 hover/active 是向黑白各混一点。

**主题的名字是文案，不是调色板。** `Theme` 上存的是 `labelKey`，由选择器 `t()` 出来；`theme.ts` 只管颜色。

**为什么不逐个主题手写 token 表。** 七套主题各写一份两百行 token 是七次写错 hover 与 base 不匹配的机会。推导保证七套主题的对比节奏一致。

**覆盖块的优先级是量过的，不是猜的。** iX 自己的 token 块选择器是 `[data-ix-theme=classic][data-ix-color-schema=dark]`（0,2,0），我们的覆盖块是 `html[data-ix-theme][data-ix-color-schema]`（0,2,1），与源码顺序无关地胜出。

**内置七套：** Windows Dark（默认）、Windows Light、Studio Dark（石墨 + 紫）、Studio Light（纸白 + 靛）、Amber Console（近黑 + 琥珀，暗房用）、Frost（冷蓝灰，低眩光）、Tokyo Night（深蓝 + 蓝紫强调色）。

**默认是 Windows 那两套，不是 iX 原样。** 应用和资源管理器、设置并排跑的时候，Fluent 的 `#202020`/`#f3f3f3` 让整个桌面看起来是一套东西；iX 原样的深青色在这台机器上被用户点名「丑」，那两套（Classic Dark / Classic Light）连同 `palette: null` 一起删掉了。删掉之后 `palette` 不再可空，每套主题都必须带一组颜色，`applyTheme` 也只剩「写覆盖块」一条路径。

**中性 ramp 可以按主题覆盖。** `RAMP` 是从背景混到前景的八个落点，参考间距来自 iX 自己的深色块。Fluent 的表面离窗口底只有几个百分点（`#202020` 窗口 → `#2b2b2b` 卡片），用参考间距会把侧栏推成中灰，所以 `windows-*` 各带一份更平的 `ramp`；Tokyo Night 用自己的 `bg_dark` / `bg_highlight`，保持 `RAMP` 不动。

**Tokyo Night 的语义色用它自己的。** 红 `#f7768e`、绿 `#9ece6a`、黄 `#e0af68`、蓝 `#7aa2f7`、紫 `#bb9af7`，不是从主色推导的；强调色是蓝、动态色是紫，和上游 vim/nvim 里「蓝做链接、紫做常数」的分工一致。

**对比度是量出来的，不是看出来的。** `tools/check-themes.mjs` 对每套主题跑 WCAG 比值：正文 4.5:1，元信息 3:1。第一版 soft-text 用 0.66 alpha，在 Studio Dark 的侧栏底色上是 3.99:1，被这个检查抓到；现在 0.78/0.58 是**通过检查的那组数字**，不是看着顺眼的那组。

**主题只有一个拥有者。** 存在 `localStorage` 的 `mio:theme`（给 `index.html` 的头部脚本读，首帧就带主题），**不**再复制进设置 blob。第一版两处都存，重载后设置里的旧值赢，主题静默回退到默认——`check-themes.mjs` 的重载断言抓到了。

**canvas 要自己跟着换。** 波形只画一次，主题切换不会自动重绘，所以 `WaveformDock` 用 `MutationObserver` 盯根元素的 `data-mio-theme` / `data-ix-color-schema`。

## 详情面板

主面板右侧一个可折叠的 Inspector：`main-head / main-body（SoundTable + Inspector）/ WaveformDock`。

**关闭时只渲染 26px 的竖排窄条。** 它是个 `button`，不是装饰：面板关掉之后，那是唯一回到面板的路。竖排标题的上下各有一个朝左的小尖号，指向面板拉出的方向；只有一个会被读成滚动箭头。打开后窄条消失，面板头上有收起按钮，`aria-expanded` / `aria-controls` 两侧都写对。宽度默认 300、夹在 240–480 之间，左边缘可拖；`mio:inspector-open` / `mio:inspector-width` 各存一份。

**它是布局列，不是 overlay。** 所以表格变窄走 CSS grid + `ResizeObserver`，不重查、不重建 segment。窄窗口里 `max-width: min(480px, 30vw)`，否则 600px 窗口会被面板吃掉一半。

**点击语义是本轮真正的产品决定，全部在 `App` 里。** Inspector 只渲染值、调用回调；它不改 `queryTerms`，不做过滤器。两个分支：

- **TEXT 追加**：文件名（去扩展名的 stem）、description、`matchedTerms`、可搜索的原始标签值 → 加进当前查询词，保留已有词与过滤器。值里没有字母数字就不做（会生成空 FTS 查询）；大小写不敏感去重。
- **清空后应用**：分类/子类、UCS 分类/子类/CatID、文件夹、封面 → 先 `clearSearchState()`（一次状态重置，**不在这里查询**），再设目标值，最后只调一次 `runSearch()`。这里刻意不复用侧栏的 `toggleCategory` / `setFolderFilter`：它们各自清不同的邻居（那是一次点击侧栏行的含义），拼起来会丢掉这次点击没提到的过滤器，而且每步查一遍。
- **文件夹是绝对路径**：`SoundRow.folder` 相对库根，后端过滤比较的是绝对路径区间；点击时从 `path` 取目录，不能用相对值。
- **侧栏跟着走**：分类/子分类/UCS/文件夹的点击把侧栏切到对应的树，并滚动到刚点亮的那一行（`SidebarReveal`，`seq` 让同一个值再点一次也会重新滚）。这里**不**走 `changeSidebarMode`——那个函数会清掉离开那棵树时的筛选，等于撤销这次点击。CatID 不参与：它没有对应的树行，筛选状态由 chip 表达。
- **数值**（声道 / 采样率 / BPM）追加：每个字段一个值，点不同值替换、点同值取消，其他过滤器不动。**完整路径、时长、大小只复制**，不改变搜索。
- **`active` 在 `clearSearchState` 里保留**：被点的文件按定义就在新结果里，清掉会让面板在用户正看着的那一行上消失。

**"同封面"是图片字节的精确哈希。** `sha256(b"mio-cover\0" + bytes)` 取前 16 字节。同一张 JPEG 嵌在 MP3 和 FLAC 里算同一张，重新编码或改分辨率的不算——这是确认过的取舍，不做感知哈希。

**原始标签按来源分组**（ID3 / Vorbis / M4A / bext / iXML / LIST INFO），已知 key 本地化，未知 key 原样显示四字码或 frame id。`searchable=false` 的值渲染成普通文本，不是按钮：UI 邀请点击而 FTS 里永远搜不到，那是个静默返回零行的过滤器。

**详情 pass 没跑到时面板懒读当前文件。** DTO 的 `lazy` 明说这一点：这些值显示得出来，但**还不在搜索索引里**。这是可接受的过渡状态，不能当成完整索引来宣传。

**`coverEpoch` 与 `dataEpoch` 分开。** 前者在 pass 完成/切库时 +1，只让缩略图与面板重取；后者会丢掉每个缓存 segment 重查列表。新缩略图不改变任何一行的成员或顺序，不该付一次查询。

**表格封面列是固定 36px、不可拖动，可隐藏。** 24px 图在 28px 行上，`.cell.cover` 自己拥有内边距（默认 8px 会把它裁掉）。缩略图**只对可见行批量请求**（`get_cover_thumbnails`，最多 128 个 key，只读 `covers` 表），列隐藏时零请求；前端 Map 上限 1024 条。点击封面格 `stopPropagation()` 后走"清空 + 封面过滤"，不触发行选中/试听。

**48px 缩略图入库，512px 面板图不入库。** 面板图从源文件解出来放 `AppState` 的 64 条 LRU（key 是 path + 文件 `(mtime, size)`，文件被重写就不会回放旧图）。给每张封面再存一份 512px BLOB 会把库数据库撑大，而只有一块面板在看它。

## 输出音量

波形 dock 里 `AUTO PLAY` 左边一组：静音按钮 + 横向滑杆（104px）+ 百分比读数。

**增益在引擎里，不在前端。** `Shared` 里一个 `AtomicU32` 存 `f32` 的位模式；渲染回调读到非单位增益时对拷进 scratch 的字节按设备采样格式原地缩放（f32 直接乘，i16/i32 用饱和转换）。代价是一次原子读加一次乘法，**不重新解码、不重新发布**，所以拖滑杆时正在响的那一段立刻变。

**单位增益走零开销路径。** 增益等于 1.0 时跳过缩放，回调仍是纯 memcpy。

**换设备时增益要带过去。** 新引擎从单位增益起，`open_audio` 在替换前把记住的值写进新引擎——否则切设备会静默把音量重置，而滑杆还显示旧位置。

**没有设备时也记得住。** `AppState` 在音频锁之外另存一份 `gain_bits`，设备打不开时设置仍然成功并返回值，下次开设备时生效。

**电平是输出的属性，不是 clip 的属性。** 没有加载任何声音时滑杆也能动，所以它永不 disabled。

## 打包与发版

产物只有安装版一种：

| 产物 | 怎么来 | 里面是什么 |
| --- | --- | --- |
| `Mio-Sound-Manager_<ver>_x64-setup.exe` | `tauri build --features asio --bundles nsis` | NSIS 安装器，当前用户安装、不弹 UAC、三语安装界面 |

**便携版从 0.1.2 起不再产出。** `tools/package-portable.sh` 留在仓库里是为了能重建 0.1.1 和更早的包；它当年存在的理由是「单 exe 只是 `cargo` 产物的改名，AGPL 要求许可与第三方声明跟着走」，于是打成 zip 把内容说清楚。现在只发安装器，这条理由落到了安装器身上：NSIS 安装包自带许可页，`THIRD-PARTY-NOTICES.md` 作为 release 附件单独挂着。

没有 DLL：`tauri` 静态链 WebView2Loader，webview 资源经 `custom-protocol` 编进 exe。

**版本号有两处，必须同步。** `Cargo.toml` 的 `[workspace.package] version` 决定 crate 版本与窗口标题里的版本，`crates/mio-app/tauri.conf.json` 的 `version` 决定安装包文件名。只改前者会得到「exe 是 0.2.0、安装包叫 0.1.1」。

**应用图标来自 `logo/mio-sound-manager.png`，由 `tauri icon` 生成。** 源图是 1000×830 带透明背景，不是正方形，`tauri icon` 会直接拒绝。`tools/make-app-icon.py` 把它居中放进 1024×1024 的透明画布（占画布 86%，与 Windows 自家图标的留白相当），然后：

```bash
python tools/make-app-icon.py
node ui/node_modules/@tauri-apps/cli/tauri.js icon logo/app-icon-1024.png -o crates/mio-app/icons
```

**`logo/` 不进仓库**（`.gitignore` 里）。构建链一个字节都不读它：`tauri build` 只认已提交的 `crates/mio-app/icons/`，顶栏标记是已提交的 `ui/src/assets/brand.svg`。所以干净克隆照样能编译、能出安装包，只是**跑不了上面这两条命令**——换品牌图重新生成图标要先自己把源图放回 `logo/`。这是有意的取舍：设计源（`.figcraft`）丢了无法从 png 反推，但它属于本机素材，不是仓库内容。

它会顺手产出 `android/` 与 `ios/` 两棵目录，本项目只做 Windows，生成后删掉即可（它们不进 `tauri.conf.json` 的 `icon` 列表，留着只是仓库噪音）。`bundle.icon` 只列 32/128/128@2x/icns/ico 五项；Windows 任务栏、开始菜单和资源管理器读的是 exe 里嵌入的 `icon.ico`，安装包图标同理。

**构建前必须关掉正在运行的应用。** Windows 不允许替换已映射的 exe，报的是 `failed to remove file … (os error 5)`——看起来像权限问题，实际是文件锁。`tools/kill-app.sh` 同时结束 `mio-sound-manager`（cargo 产物与安装版）与 `Mio Sound Manager`（0.1.1 及更早的便携版 exe）两个进程名。

**`tauri build` 要在仓库根执行。** `ui/` 下没有 `tauri.conf.json`，在 `ui/` 里跑会报 `Couldn't recognize the current folder as a Tauri project`。

发版流程、附件清单与 `check-shipped.mjs` 的用法见 `tools/README.md` 的「发版」一节——那里是唯一需要维护的那份。

## 验证

**Rust 侧测试不需要窗口**（`cargo test --release --workspace`；测试数会随提交变化，以命令输出为准）：

- `tests/ipc.rs` 25 个，跑在真实设备上（本机 FxSound；`MIO_TEST_DEVICE` 可换）。P3/P4 部分覆盖：状态报真实设备格式、设备缺失报引擎原文、播放推进播放头、`startAt` 偏移、同文件重播不重新解码、被取代的播放不播旧音频、非音频路径提前拒绝、波形 1000 点且响/静可分辨、桶数夹住、文件夹树来自索引、无库时报错可显示；P5 新增 6 个——两个库同时打开且切换后搜索只看活跃库、关掉活跃库自动落到下一个、切到未打开的库被拒、undo 恢复收藏且 redo 放回、undo 恢复的是**旧分类和旧归属**、多行编辑一次撤销。
- `commands.rs` 6 个：时长标签格式（含 59.999s 进位）、排序白名单、分页上限钳制。
- `session.rs` 5 个、`audio_config.rs` 4 个、`history.rs` 3 个：会话往返与损坏降级、设备选择持久化、undo/redo 栈上限与 `EditKind`。
- `mio-index`、`mio-engine` 的单测（含 3 个选区导出：只保留自己那段的帧、超出末尾被夹住、倒置区间写出合法的空 WAV）。

**真实 28 万文件库（`D:/reslip-test-library`，本机默认设备 FxSound）：**

| 口径 | 实测 |
| --- | --- |
| `add_library`（增量，返回即完成） | **11.6 s** |
| 搜索 `gunshot` | 38 行，约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| Ctrl+A 全选 | **271,350** 条路径 |
| 无限滚动到 1500 行 | 6 次滚到底，全部追加成功 |

**窗口内交互用 CDP 驱动真实窗口验证，不靠人看**（先 `./tools/run-app.sh` 起窗口，再跑脚本）：

- `tools/check-interactions.mjs`——列表渲染、点击选中与激活、Ctrl/Shift 多选、状态栏计数、点列头排序并翻转方向、搜索过滤与空结果、右键菜单、分类 modal 打开/尺寸/Escape 关闭、`AUTO PLAY` 文字与切换并持久化（含「未覆盖设置前默认是开」的回归断言，脚本结束把 profile 还原）、播放头推进/暂停冻结/Stop 归零、波形点击 seek、设置 modal 与主题卡片。
- `tools/check-region.mjs`——行真的可拖（`row.draggable === true` 且 `draggable="true"`）、波形拖拽选出区域、循环开关读到引擎且带选区、Esc 清选区后循环回落到整个文件、`export_region` 写出 WAV 并返回路径、最后真派发一次 `dragstart`。**这一份跑完 webview 会因原生 OLE 拖拽而不再响应 IPC**，必须单独跑、跑完重启应用。它需要 AUTO PLAY 为开（点击行才加载 clip），脚本自己打开并还原 `mio:settings`。
- `tools/check-scroll-deep.mjs`——列表滚到 10/30/50/80/95% 各跳一次，断言每次都在 5s 内绘出**该偏移处**的真实行。真实 289k 库实测 134–217ms；一万行以上的合成库也能跑。
- `tools/measure-details.mjs` / `measure-details2.mjs`——§8.2 的测量：批量缩略图、`inspect_sound` 冷/热、封面过滤与排序、FTS 标签查询。需要窗口开着且库已跑完 details pass。
- `tools/lib-stats.py` / `lib-size.py` / `lib-tagprobe.py`——只读统计（行数、schema/fts 版本、封面计数、表体积、标签文本字节），一律 `mode=ro` 打开。
- `tools/check-themes.mjs`——七套主题逐一应用，断言 DOM 里的 token 真的变了、每套互不相同、背景与文字的高低关系符合该主题的 schema（按 `data-ix-color-schema` 判，不按 id 猜）、覆盖块真的注入了且不是空块、**存着已删除的 `classic-dark` 时回退到默认**、重载后主题仍在，每套主题各自跑一遍十处文本元素的 WCAG 对比度，以及 AUTO PLAY 开态的 primary/contrast 对比度。
- `tools/check-volume.mjs`——滑杆经 `valueChange` 改值后读**引擎的** `get_gain`（另一条命令，只改前端状态的实现过不了这一项）、静音→取消静音回到设定值（不是 100%）、静音后重载仍回到那个值、重载后电平仍在且被推回引擎。
- `tools/check-layout.mjs`——五个区域的位置尺寸、表头与行列轨道对齐（容差 0.5px）、单元格与表头不被裁切、dock 控件簇的**顺序**（音量 → AUTO PLAY → 循环 → 暂停 → 停止）与间距、音量滑杆宽度、无元素逃出视口（滚动容器内的不算）。
- `tools/check-tags.mjs`——侧栏模式下拉（存储、旧键迁移、切模式后的键盘导航）、库头无折叠三角与活跃态、文件夹行右键菜单、分类多选、BPM 列与排序、列拖动重排（表头与单元格同源、持久化、拖动后仍能排序、收藏星不可拖）、列宽拖动与重载保留、设置页与表头菜单的列显隐、表头右键菜单内容、横向滚动后的表头/行对齐、索引卡片无解释小字、界面缩放在 `innerWidth` 上真的生效、设置页无解释小字/无副标题。
- `tools/check-i18n.mjs`——三语字典的键集与占位符一致性（`tsc` 管不到的那部分）。
- `tools/check-inspector.mjs`——详情面板的布局与点击语义（可以对着 Vite dev server 的 fixture 跑，也可以对 `9223` 的真实窗口跑）：窄条 → 面板、`aria-expanded`、宽度持久化与拖动、表格与面板边界、TEXT 追加 vs 清空后应用、分类/文件夹/声道 chip、复制路径不重查；fixture 模式下另测封面列（表头顺序、24px 绘制、只有可见行的批量请求、隐藏列零请求）、`inspect_sound` 的标签分组与 `searchable` 按钮、封面点击的 `coverHash` 过滤。
- `tools/check-inspector-live.mjs`——对着**真实文件**（`tools/make-inspector-fixture.py` 造的小库）验证另一半：内嵌 APIC 与 sidecar 封面各自产出缩略图、面板解的是 320px 源图而不是 48px 缩略图、ID3/bext/iXML/LIST 的标签都在、标签点击成为查询词、封面过滤只剩一行。
- `tools/check-shipped.mjs`——对着**打包产物**而不是 `target/release`：解压 zip、用调试端口起那个 exe，断言窗口标题、`asio_compiled=true`、引擎开到设备、`#transport-loop` / `#volume-mute` / `.dock-hints` 存在、`get_loop` 有应答。对着开发目录跑它会漏掉打包环节的问题。
- `tools/shot.mjs` / `shot-themes.mjs`——截图，给人看，不做断言。

**详情/封面的测试库必须自己造。** `tools/make-inspector-fixture.py` 写一个 19 文件的小库（带 bext + iXML + LIST + ID3 与内嵌 PNG 的 WAV、sidecar `cover.png`、普通 WAV，其中两个文件名带 BPM），因为它既要有封面、有标签、有 BPM，又要足够长（5s）让播放/通道的回归断言仍然成立。**开发性的 details pass 不碰真实库**：要么用这个 fixture，要么用真实库的副本（只复制数据库，音频用硬链接或直接接受播放不可用），原库全程只读。`check-scroll-deep.mjs` 一万行以上就能跑。

**行缩略图必须走 `get_cover_thumbnails`。** 每个 cell 自己 `invoke`、或者滚动时打开音频文件，都会在 289k 库上变成每帧几十次文件 IO；`check-inspector` 数的是批量请求次数与 key 的集合，不是"看起来有图"。

**CDP 脚本固定英文 locale 再断言。** 六份会驱动窗口的 `check-*.mjs` 断言的文案是英文（`Length`、`AUTO PLAY`、`Resume`），在中文机器上会因为找不到 locator 而失败——那是脚本的前提问题，不是功能坏了。所以脚本开头写 `localStorage.setItem('mio:locale', 'en')` 再 reload；列头和菜单项另有 `data-sort-key` / `data-menu-id`，选元素时不依赖翻译。文案覆盖由 `check-i18n.mjs` 负责。

**驱动 iX 组件的三条经验**（写脚本时都会踩到）：

1. `host.click()` 没用，控件在 shadow root 里；要点 `host.shadowRoot.querySelector('button')`。
2. 禁用的 iX 按钮有 `pointer-events: none`，Playwright 会一直重试到超时；先断言状态再点内层。
3. 上一轮留下的 modal 会挡住后面所有点击——脚本开头先 `page.reload()`。

## 踩到的坑

**`tauri` 必须开 `custom-protocol`。** 不开的时候 release 构建仍然把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是一片空白窗口——第一次运行正是这样，页面 URL 是 `http://localhost:1420/` 而不是 `http://tauri.localhost/`。开了之后 asset 协议从 exe 内部取文件。

**CSP 必须显式放行 `connect-src`。** P3 的 CSP 只写了 `default-src 'self'`，于是每次 IPC 都会先被 CSP 拦一次再到 `ipc.localhost`——控制台里两条错误，功能靠 Tauri 的兜底路径还能跑。P4 加了 `connect-src 'self' ipc: http://ipc.localhost`。

**`Channel` 不能是 `Option`。** `Option<tauri::ipc::Channel<T>>` 不实现 `Deserialize`，写成可选参数会编译失败。命令签名里它必须必填。

**`State<'_, T>` 不能跨进 `spawn_blocking`。** 它借的是命令的栈帧。要在线程里用状态就拿 `AppHandle` 再 `app.state::<T>()`。

**Windows 不允许替换正在映射的 exe。** `cargo build --release` 会在 `failed to remove file mio.exe` 处失败。`tools/kill-app.sh` 用 PowerShell 的 `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

**想让 UI 可验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。`playwright-core` 不是本仓库的依赖，它装在参考项目 `slip-sound/` 里；验收脚本用 `createRequire` 指向那个 `package.json`，因此可以从任何目录运行。

**未收藏的星标单元格必须画出字形。** 空 span 宽度为零，点击目标不存在——于是"收藏"只能在已经收藏之后才点得动，等于从行上无法收藏。现在未收藏时画 `☆`，颜色退到边框色。

**iX 的 CSS 是 254KB 的一份文件。** `@siemens/ix/dist/siemens-ix/siemens-ix.css` 里既有组件样式也有两套主题 token 块，`defineCustomElements()` 之后组件才真正注册。只 import CSS 不注册元素，页面上只有一堆无样式的空标签。

**Siemens Sans 本机没有。** 字体链退到 `Segoe UI Variable Text` → `Segoe UI` → `Arial`，比例接近，不额外打包字体文件（iX 的字体是商业授权）。

## 已知未做

- 分类侧栏的折叠状态不持久（展开/收起在会话内有效，重启回到全折叠）；文件夹树的展开状态、侧栏模式、宽度、设置项都是持久化的。
- 长文件仍整段解码（P1 取舍）；波形因此也是一次整段解码，`decodeMs` 直接显示给用户。
- undo 不跨重启，也不覆盖「删除」「改名」「重索引」。
- 主题只在 `localStorage`，没有「导入/导出一份主题」的界面；加一套主题要改 `theme.ts` 的 `THEMES` 数组。
- 音量滑杆是线性的，没有做 dB 曲线；`0..100%` 直接映射到增益 `0..1`。
- 拖拽时悬停的光标样式不变（P0 用户指出过）。
- 不做 Slicer / Effects / Export，也不做 macOS / Linux 版本。ASIO 后端曾按此逻辑被移出，用户试用后要求，P6 已实现（见 `docs/engine.md`）；P10 加的循环是试听用的循环播放，不是 Loop 工具（切片后重排）。
