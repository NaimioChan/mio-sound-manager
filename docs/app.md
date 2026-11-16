# ReSlip 桌面外壳

`crates/reslip-app` 的模块细节。架构依据见 `docs/architecture.md` 第 4、7 节；本文件说明外壳怎么切、每个决定为什么这么做、以及验收是怎么测的（不是靠人眼看窗口）。

## 模块

```
crates/reslip-app/
  src/main.rs        // 入口，release 下隐藏控制台
  src/lib.rs         // Tauri builder、invoke_handler、窗口
  src/commands.rs    // IPC 线上类型 + 每个命令的薄壳
  src/state.rs       // 引擎、解码 worker、库句柄、编辑操作
  src/library.rs     // 索引生命周期、后台索引线程、批量写入
  src/error.rs       // AppError → 字符串，跨 IPC 边界
  tauri.conf.json    // 窗口、asset 协议、CSP
  capabilities/      // ACL：core:default + drag + dialog + opener
  tests/ipc.rs       // 不打开窗口就能跑完的验收
ui/                  // SolidJS + Vite，构建产物被编进 exe
  src/App.tsx            // 状态、搜索、选择、快捷键、菜单装配
  src/SoundTable.tsx     // 虚拟列表、多选、键盘、拖拽起点
  src/Sidebar.tsx        // 分类 facet / 文件夹树两模式
  src/WaveformDock.tsx   // 波形、播放头、暂停
  src/ContextMenu.tsx    // 右键菜单
  src/CategoryDialog.tsx // 手动分类
  src/ipc.ts             // 线上类型 + 命令封装
tools/               // run-app / kill-app / CDP 驱动与验收脚本
```

## 三层，各自的规则

**命令层不决定任何事。** `commands.rs` 里的每个 `#[tauri::command]` 只做三件事：校验参数形状、调用 `AppState` 的方法、返回线上类型。逻辑放在 `state.rs`，因为这一层能用 `cargo test` 直接驱动——`tests/ipc.rs` 里的 19 个测试全部走真实的 `AppState`，不经过窗口。只有靠人点按钮才能验证的外壳等于没验证。

**引擎只有一个实例，进程级常驻。** WASAPI 流在 `setup` 里打开，直到进程退出。这跟 P1 的取舍一致：共享模式的 12ms 排队是打开流就付掉的，反复开关只会更慢。设备打不开时应用照常启动，`get_status` 报 `audioAvailable: false` 与引擎自己的错误文本（带设备名或格式），播放命令返回同一条错误——一个坏设备不该让窗口起不来。

**解码在专用 worker 上，队列是"后到者赢"。** 3 分钟 96kHz 文件解码 112ms，跑在命令线程上会把 webview 卡住。`LatestSlot` 是一个只存一件活的信箱：新请求替换排队中的旧请求，被替换的那个收到 `superseded`。按住方向键扫过一个列表时，只解码最终停下的那一行，而不是路过的每一行。

**播放头是纯原子读。** `get_playback_position` 只做几个 `AtomicU64` 载入，不取 `arc-swap` guard、不加锁、不分配——它每秒被调 60 次。`active_frames` 是跟 clip 一起发布的，所以时长也不需要碰槽位。

**编辑走批量事务。** 收藏、设分类、清分类都是 `*_many` 形式，一个事务写完整个选区。单行版本保留给星标单击，但多选 200 行时逐行提交等于 200 次 WAL fsync。

## 引擎的四处改动（P1 → P4）

1. **`publish` / `publish_from` / `stop` 改 `&self`。** 它们只写原子量与 `arc-swap`，不需要 `&mut`。改完之后任何线程都能直接发布，命令层不必再搭一个控制线程来串行化。
2. **`publish_from(clip, offset_frames)`。** IPC 的 `start_at` 落到这里。seek 是"在不可变缓冲上换起始位置"，代价与普通播放相同的一次原子写，不重新解码、不裁剪。
3. **`PlaybackMonitor`。** `Engine::monitor()` 克隆一份 `Arc<Shared>` 加格式信息，给 webview 轮询用。`drain_reports()` 也在上面，所以状态栏能显示**实测**的 `publish → 出声`，而不是文档里的目标值。
4. **`pause` / `resume`（P4）。** `stop` 清槽并把播放头归零，这是 Stop 该做的；空格需要的是原地冻结。`pause` 只翻一个 `AtomicBool`，渲染回调看到它就写静音、不动 `pos_frames`，clip 留在槽里，所以 `resume` 是一次原子写，不重新解码、不重新 seek，播放头从听众停下的那一帧继续。`publish` 会清掉这个标志——留着它会让新发布的音频静音播放，那是唯一看起来像引擎坏掉的状态。暂停期间也不计下溢：槽里还有 clip，写静音是正确行为。

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
| `open_library(root)` | `LibraryStatus` | 返回时库可读，索引在后台跑 |
| `get_library` | `LibraryStatus?` | 索引进度轮询 |
| `search(request)` | `SoundDto[]` | 同 `reslip-index` 的过滤条件，上限 5000 行 |
| `count_matching(request)` | `u64` | 当前过滤命中总数，状态栏的 "38 of 27,525" |
| `list_matching_paths(request)` | `string[]` | 全选用的纯路径列表，不受 5000 行上限 |
| `list_folders` | `FolderNodeDto` | 从索引构建的树，不读 `readdir` |
| `get_sidebar` | `SidebarDto` | facet 计数 + 收藏数 + 总数，一次往返 |
| `set_favorite(paths, favorite)` | `u64` | 一个事务 |
| `set_category(paths, category, sub?)` | `u64` | 置 `category_manual`，重索引不覆盖 |
| `clear_category(paths)` | `u64` | 交还分类器 |
| `pick_library_folder` | `string?` | 原生目录选择，取消返回 `null` |
| `reveal_in_folder(path)` | `()` | Explorer 里选中该文件 |

线上字段一律 camelCase（`#[serde(rename_all = "camelCase")]`），前端类型手写在 `ui/src/ipc.ts`，与 Rust 侧一一对应。

`search` 与 `list_matching_paths` 分开是刻意的：前者返回完整行且限 5000（一页没有更大的理由），后者只回路径、不设上限。28 万库里 Ctrl+A 要选的是 28 万条路径，不是 5000 条——用 `search` 加个大 limit 会在 5000 处静默截断，而"全选"少选了比没有这个快捷键更糟。

## 前端交互（P4）

**选择是锚点 + 集合。** 点击替换、Ctrl+点击切换、Shift+点击从锚点扩展。锚点存的是**路径**不是下标：列表在搜索和排序下会重排，下标会指向另一个声音。

**列表按行窗口虚拟化。** 索引能返回上千行，固定行高让窗口计算是精确的——行高一旦量不准，滚动时就会跳。表头在滚动容器之外，所以它的列轨道要减去滚动条宽度；用 `width: calc(100% - Npx)` 而不是 padding，因为 `box-sizing: border-box` 下 padding 会让 `fr` 轨道按比例吸收差值，两个网格反而偏出两倍滚动条的距离。

**行必须 `left: 0; right: 0`。** 绝对定位的盒子默认收缩到内容宽度，不设左右边界时网格轨道是按文字宽度算的，列会和表头错开。

**键盘归列表。** 上下/Home/End/PageUp/PageDown 移动并试听，Ctrl+A 全选当前过滤，空格暂停/继续。绑定在 window 上但输入框有焦点时跳过，否则在搜索框里打字会逐个字母试听。

**拖拽导出用 `preventDefault` 拦住 DOM drag。** Tauri 的拖拽插件自己发起原生 OLE 拖拽，让 WebView2 同时再起一个 DOM 拖拽会让系统看到两个竞争的拖拽源。插件命令的 `onEvent` 是 `Channel` 且**必填**——不传整条命令直接报 `missing required key onEvent`，拖拽根本不会开始。P0 已实测这个通道从不触发，传它是为了满足签名。

## 验证

**Rust 侧 83 个测试，不需要窗口**（`cargo test --release --workspace`）：

- `tests/ipc.rs` 19 个，跑在真实设备上（本机 FxSound；`RESLIP_TEST_DEVICE` 可换），覆盖：状态报的是真实设备格式而非默认值、设备缺失时报引擎原文、播放推进播放头、`startAt` 偏移生效、同文件重播不重新解码、被取代的播放不播旧音频、非音频路径在解码前拒绝、缺文件报错不挂起、波形是整段 1000 点且响/静两半可分辨、桶数上下限被夹住、打开库后台索引后搜索命中、文件夹树来自索引且空文件夹不出现、无库时错误可显示、重复打开同一库不启第二个索引器、不存在的文件夹被拒；P4 新增 4 个——收藏批量写入后行与侧栏数字一致、手动分类能扛住一次全量重索引而清除后交还分类器、facet 计数覆盖全部行、暂停冻结播放头且 resume 从同一帧继续。
- `commands.rs` 7 个，覆盖时长/声道标签格式、排序白名单（未知列名报错而不是拼进 SQL）、分页上限钳制。
- `reslip-index` 44 个、`reslip-engine` 13 个。

**真实 28 万文件库（`D:\reslip-test-library`，本机默认设备 FxSound）：**

| 口径 | 实测 |
| --- | --- |
| `open_library` → 首屏有行 | **69 ms** |
| 搜索 `gunshot` | 38 行，约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| Ctrl+A 全选 | **271,350** 条路径 |
| 增量索引全程 | **6.5 s**（与 P2 的 5.4s 同量级，差异是这里走 `latest` 顺序） |

**交互用 CDP 驱动真实窗口验证，不靠人看：**

- `tools/check-browsing.mjs`（40 项）——打开库、分类筛选与 chip、方向键移动、Shift/Ctrl 多选、Ctrl+A、右键菜单与 Escape、星标写入数据库、分类对话框存取与清除、排序箭头、点击到波形 32–36ms、空格暂停时播放头 0.61→0.61s、继续后 0.61→1.01s、Stop 归零、拖拽命令到达插件、文件夹模式。
- `tools/check-layout.mjs`——按真实 DOM 几何断言：五个区域的位置与尺寸、表头与行的列轨道对齐（容差 0.5px）、每个单元格与表头文字不被裁切、dock 头部子元素不重叠、没有元素溢出视口。
- `tools/check-interactions.mjs`（P3）——播放头推进、点波形 seek、Stop 后钉住。

## 踩到的坑

**`tauri` 必须开 `custom-protocol`。** 不开的时候 release 构建仍然把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是一片空白窗口——第一次运行正是这样，页面 URL 是 `http://localhost:1420/` 而不是 `http://tauri.localhost/`。开了之后 asset 协议从 exe 内部取文件。

**CSP 必须显式放行 `connect-src`。** P3 的 CSP 只写了 `default-src 'self'`，于是每次 IPC 都会先被 CSP 拦一次再到 `ipc.localhost`——控制台里两条错误，功能靠 Tauri 的兜底路径还能跑。P4 加了 `connect-src 'self' ipc: http://ipc.localhost`。

**Windows 不允许替换正在映射的 exe。** `cargo build --release` 会在 `failed to remove file reslip.exe` 处失败。`tools/kill-app.sh` 用 PowerShell 的 `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

**想让 UI 可验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。`playwright-core` 不是本仓库的依赖，它装在参考项目 `slip-sound/` 里；验收脚本用 `createRequire` 指向那个 `package.json`，因此可以从任何目录运行，不必先 `cd` 过去。

**未收藏的星标单元格必须画出字形。** 空 span 宽度为零，点击目标不存在——于是"收藏"只能在已经收藏之后才点得动，等于从行上无法收藏。现在未收藏时画 `☆`，颜色退到边框色。

## 已知未做

- 分类侧栏的折叠状态不持久（展开/收起在会话内有效，重启回到全折叠）；侧栏模式（分类/文件夹）是持久化的。
- 长文件仍整段解码（P1 取舍）；波形因此也是一次整段解码，`decodeMs` 直接显示给用户。
- 无限滚动未做：一页 500 行，到页尾需要改过滤条件或全选。参考实现有滚动加载。
- 撤销/重做未做：收藏与分类写入后不可撤回（参考实现有 undo 栈）。
- Slicer / Effects / Loop / Export 属于 P5。
- 拖拽时悬停的光标样式不变（P0 用户指出过，留到产品阶段）。
