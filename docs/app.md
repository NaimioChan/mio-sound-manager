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
  src/WaveformDock.tsx   // 波形、播放头、暂停、输出音量
  src/ContextMenu.tsx    // 右键菜单（视口坐标，非 iX dropdown）
  src/CategoryDialog.tsx // 手动分类（iX modal）
  src/SettingsDialog.tsx // 设置页（iX modal，含主题选择）
  src/IndexOverlay.tsx   // 阻塞式索引的遮罩
  src/theme.ts           // 配色主题系统：调色板 → iX token 覆盖
  src/icons.ts           // 图标 SVG（仓库内 src/assets/icons/），转 data URL 给 ix-icon
  src/i18n.ts            // 语言信号 + t()；locale 存 mio:locale
  src/locales/{en,zh,ja}.ts // 三语字典，en 是键集基准
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
| `get_waveform(path, buckets?)` | `WaveformDto` | ~1000 个 f32，附带解码耗时 |
| `add_library(root, full?, onProgress)` | `LibraryStatus` | **阻塞**到索引跑完；进度走 Channel |
| `list_libraries` | `LibraryStatus[]` | 每个打开库的名字、计数、是否活跃 |
| `remove_library(root)` | `LibraryStatus[]` | 关掉一个库，活跃的自动落到下一个 |
| `set_active_library(root)` | `LibraryStatus[]` | 切换显示哪个库 |
| `reindex_library(root?, full?, onProgress)` | `LibraryStatus` | **阻塞**，同上；`root` 缺省是活跃库 |
| `search(request)` | `SoundDto[]` | 同 `mio-index` 的过滤条件，上限 5000 行 |
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

**侧栏宽度可拖。** 把手是 `.rail` 里的绝对定位元素（`right: -3px`），不是 `.body` 的第三个 grid 子元素——那会多出一列并把表格挤走。宽度写进 `localStorage`，范围 180–520px。

**搜索框在列表列顶部，不在窗口顶部。** 它过滤的是列表，所以它属于列表。窗口顶部留给「我在哪个库」、undo/redo 和设置（图标）。播放的三个控件在波形 dock 的右端聚成一组：`AUTO PLAY` 是文字按钮，暂停/继续与停止是图标——前者的状态是「点一行会不会响」这条策略，12px 的图标说不出来。

**库条目在视觉上高于它下面的树。** 库头部是 650 字重、带底色块、顶部一条分隔线，活跃库另有左侧强调条；树行是 400 字重、无底色。这不是装饰：库条目带自己的右键菜单，看起来像树的一行会让人想不到那里能右键。

**设置页三节：Audio / Playback / List。** Audio 是输出设备选择（WASAPI 端点与 ASIO 驱动合成一个下拉，ASIO 选中时多一个缓冲大小下拉）加一行当前格式与周期。Playback 只有 `Play on click`。List 是滚动加载、每页行数、分类列。库的管理动作不在这里——见「多库」。

**说明性副标题一律去掉。** 设置项下面只留两种文字：设备打不开的原因，和当前格式那一行。工具里没有需要先读一段话才能用的设置。

**排序名和行字段名是两套。** 渲染层按行字段排序（`durationS`、`sampleRate`），索引层白名单用的是 SQL 列名（`duration`、`sample_rate`）。`SORT_WIRE`（`SoundTable.tsx`）是唯一做这个翻译的地方。第一版把行字段名直接当 `sortBy` 发出去，`search` 报 `unknown sort column durationS`；而且因为失败那次之后排序值没变，后续每次搜索都带着同一个坏名字，于是点一次 Length 之后分类和文件夹筛选也一起失效——直到用户改去点 Name。

**第一页查询在进行中时 `data-querying="true"`。** 列表不阻塞：旧行还在、还能点，但查询落地时要整批换掉。这个标志既是表尾的 `searching…`，也是 `tools/` 里 CDP 脚本等待的信号——之前脚本靠 sleep 猜，猜早了点在即将被卸载的行上，选中跟着丢，看起来像点击处理器坏了。

**文件夹树按需挂载，展开状态按库持久化。** 子节点只在展开时进 DOM：真实库有 27 万个文件，全量渲染一遍再靠 CSS 隐藏，每次侧栏重绘都要建几千个节点。展开集合存在 `localStorage` 的 `mio:folders-expanded:<root>` 里，切库时跟着换——不存的话每次重绘都折叠回根，深目录根本点不进去。`.twisty` 是可点的 12px 方块（3px 宽的三角点不中），行本身点击是选筛选，双击是展开/折叠。

**类名不要用 `.empty`。** 它是整窗空状态（`padding: 40px`、`height: 100%`），任何 `.foo.empty` 都会连带命中。叶子节点那个占位 twisty 因此叫 `.twisty.spacer`。

## 西门子 iX 设计系统

前端用 `@siemens/ix` 5.3 的 web components 重写，布局与交互不变。组件层面用到的：`ix-button`、`ix-icon-button`、`ix-toggle-button`、`ix-slider`、`ix-input`、`ix-toggle`、`ix-select`、`ix-chip`、`ix-pill`、`ix-modal`、`ix-progress-indicator`、`ix-icon`。表格、侧栏、右键菜单、波形 dock 仍是自己的 DOM——iX 没有虚拟化表格，而 27 万行的列表不能交给一个每行都建组件的实现。

**颜色一律来自 `--theme-*`。** `styles.css` 里没有一个字面色值（只有几何尺寸）。这是主题系统能工作的前提：主题改一次 token，iX 组件、自有 CSS、canvas 三处一起变。

**Solid 与 iX 的接线有三处要注意：**

1. **普通属性绑定就够。** `value={signal()}` 会写成 property，signal 变化时 iX 组件跟着更新（浏览器里验过，不需要 `prop:` 前缀）。
2. **CustomEvent 用 `on:` 前缀。** `on:valueChange`、`on:checkedChange`、`on:pressedChange`、`on:closeChip`。Solid 走 `addEventListener`，`detail` 在 `event.detail` 上。
3. **`aria-label` 会被 iX 搬走。** iX 的 `InheritAriaAttributesMixin` 把 host 上的 `aria-*` 移到 shadow root 内的 `<button>` 上并**从 host 删除**。所以 CDP 脚本读标签要读内层按钮，读 host 会拿到 `null`。

**iX 元素在 Solid 里需要类型声明。** `src/ix-elements.d.ts` 给用到的每个标签声明了属性；基础类型继承 `JSX.DOMAttributes<HTMLElement>`，否则连 `onClick` 都不认。`children` 也要显式声明——Solid 的 JSX 不自动加。

**modal 是命令式打开的。** 渲染出 `ix-modal` 之后调 `showModal()`（内部 `dialog.showModal()` 加动画）。host 在 `.visible` 之前是 `display: none`，而内层 `dialog` 是 `position: fixed`，所以**量 host 的 `getBoundingClientRect()` 永远是 0×0**，检查要量 shadow root 里的 `dialog`。

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

**主题的名字与说明是文案，不是调色板。** `Theme` 上存的是 `labelKey` / `hintKey`，由选择器 `t()` 出来；`theme.ts` 只管颜色。

**为什么不逐个主题手写 token 表。** 六套主题各写一份两百行 token 是六次写错 hover 与 base 不匹配的机会。推导保证六套主题的对比节奏一致。

**覆盖块的优先级是量过的，不是猜的。** iX 自己的 token 块选择器是 `[data-ix-theme=classic][data-ix-color-schema=dark]`（0,2,0），我们的覆盖块是 `html[data-ix-theme][data-ix-color-schema]`（0,2,1），与源码顺序无关地胜出。

**内置六套：** Classic Dark（iX 原样）、Classic Light（iX 反色）、Studio Dark（石墨 + 紫）、Studio Light（纸白 + 靛）、Amber Console（近黑 + 琥珀，暗房用）、Frost（冷蓝灰，低眩光）。`classic-*` 的 `palette` 是 `null`，也就是**不注入覆盖块**，这是「iX 原样」的实现方式。

**对比度是量出来的，不是看出来的。** `tools/check-themes.mjs` 对每套主题跑 WCAG 比值：正文 4.5:1，元信息 3:1。第一版 soft-text 用 0.66 alpha，在 Studio Dark 的侧栏底色上是 3.99:1，被这个检查抓到；现在 0.78/0.58 是**通过检查的那组数字**，不是看着顺眼的那组。

**主题只有一个拥有者。** 存在 `localStorage` 的 `mio:theme`（给 `index.html` 的头部脚本读，首帧就带主题），**不**再复制进设置 blob。第一版两处都存，重载后设置里的旧值赢，主题静默回退到默认——`check-themes.mjs` 的重载断言抓到了。

**canvas 要自己跟着换。** 波形只画一次，主题切换不会自动重绘，所以 `WaveformDock` 用 `MutationObserver` 盯根元素的 `data-mio-theme` / `data-ix-color-schema`。

## 输出音量

波形 dock 里 `AUTO PLAY` 左边一组：静音按钮 + 横向滑杆（104px）+ 百分比读数。

**增益在引擎里，不在前端。** `Shared` 里一个 `AtomicU32` 存 `f32` 的位模式；渲染回调读到非单位增益时对拷进 scratch 的字节按设备采样格式原地缩放（f32 直接乘，i16/i32 用饱和转换）。代价是一次原子读加一次乘法，**不重新解码、不重新发布**，所以拖滑杆时正在响的那一段立刻变。

**单位增益走零开销路径。** 增益等于 1.0 时跳过缩放，回调仍是纯 memcpy。

**换设备时增益要带过去。** 新引擎从单位增益起，`open_audio` 在替换前把记住的值写进新引擎——否则切设备会静默把音量重置，而滑杆还显示旧位置。

**没有设备时也记得住。** `AppState` 在音频锁之外另存一份 `gain_bits`，设备打不开时设置仍然成功并返回值，下次开设备时生效。

**电平是输出的属性，不是 clip 的属性。** 没有加载任何声音时滑杆也能动，所以它永不 disabled。

## 验证

**Rust 侧 105 个测试，不需要窗口**（`cargo test --release --workspace`）：

- `tests/ipc.rs` 25 个，跑在真实设备上（本机 FxSound；`MIO_TEST_DEVICE` 可换）。P3/P4 部分覆盖：状态报真实设备格式、设备缺失报引擎原文、播放推进播放头、`startAt` 偏移、同文件重播不重新解码、被取代的播放不播旧音频、非音频路径提前拒绝、波形 1000 点且响/静可分辨、桶数夹住、文件夹树来自索引、无库时报错可显示；P5 新增 6 个——两个库同时打开且切换后搜索只看活跃库、关掉活跃库自动落到下一个、切到未打开的库被拒、undo 恢复收藏且 redo 放回、undo 恢复的是**旧分类和旧归属**、多行编辑一次撤销。
- `commands.rs` 6 个：时长标签格式（含 59.999s 进位）、排序白名单、分页上限钳制。声道标签测试随 `channel_label` 一起删了，词改在前端。
- `session.rs` 5 个：缺文件是空会话、往返、损坏文件降级、消失的文件夹跳过但保留、活跃库优先。
- `history.rs` 3 个：新编辑清空 redo、栈有上限、`EditKind` 描述将要发生的事。
- `mio-index` 49 个、`mio-engine` 13 个、`mio-app` lib 18 个。

**真实 28 万文件库（`D:/reslip-test-library`，本机默认设备 FxSound）：**

| 口径 | 实测 |
| --- | --- |
| `add_library`（增量，返回即完成） | **11.6 s** |
| 搜索 `gunshot` | 38 行，约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| Ctrl+A 全选 | **271,350** 条路径 |
| 无限滚动到 1500 行 | 6 次滚到底，全部追加成功 |

**窗口内交互用 CDP 驱动真实窗口验证，不靠人看**（先 `./tools/run-app.sh` 起窗口，再跑脚本）：

- `tools/check-interactions.mjs`——列表渲染、点击选中与激活、Ctrl/Shift 多选、状态栏计数、点列头排序并翻转方向、搜索过滤与空结果、右键菜单、分类 modal 打开/尺寸/Escape 关闭、`AUTO PLAY` 文字与切换并持久化、播放头推进/暂停冻结/Stop 归零、波形点击 seek、设置 modal 与主题卡片。
- `tools/check-themes.mjs`——六套主题逐一应用，断言 DOM 里的 token 真的变了、每套互不相同、浅色主题的背景确实比文字亮、`classic-*` 不残留覆盖块、重载后主题仍在，最后对十处文本元素跑 WCAG 对比度。
- `tools/check-volume.mjs`——滑杆经 `valueChange` 改值后读**引擎的** `get_gain`（另一条命令，只改前端状态的实现过不了这一项）、静音/取消静音、重载后电平仍在且被推回引擎。
- `tools/check-layout.mjs`——五个区域的位置尺寸、表头与行列轨道对齐（容差 0.5px）、单元格与表头不被裁切、dock 控件簇的**顺序**（音量 → AUTO PLAY → 暂停 → 停止）与间距、音量滑杆宽度、无元素逃出视口（滚动容器内的不算）。
- `tools/check-i18n.mjs`——三语字典的键集与占位符一致性（`tsc` 管不到的那部分）。
- `tools/shot.mjs` / `shot-themes.mjs`——截图，给人看，不做断言。

**CDP 脚本固定英文 locale 再断言。** 四个 `check-*.mjs` 断言的文案是英文（`Length`、`AUTO PLAY`、`Resume`），在中文机器上会因为找不到 locator 而失败——那是脚本的前提问题，不是功能坏了。所以脚本开头写 `localStorage.setItem('mio:locale', 'en')` 再 reload；列头和菜单项另有 `data-sort-key` / `data-menu-id`，选元素时不依赖翻译。文案覆盖由 `check-i18n.mjs` 负责。

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
- 不做 Slicer / Effects / Loop / Export，也不做 ASIO 后端——已从路线图移除。
