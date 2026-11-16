# ReSlip 桌面外壳

`crates/reslip-app` 的模块细节。架构依据见 `docs/architecture.md` 第 4、7 节；本文件说明外壳怎么切、每个决定为什么这么做、以及验收是怎么测的（不是靠人眼看窗口）。

## 模块

```
crates/reslip-app/
  src/main.rs        // 入口，release 下隐藏控制台
  src/lib.rs         // Tauri builder、invoke_handler、窗口
  src/commands.rs    // IPC 线上类型 + 每个命令的薄壳
  src/state.rs       // 引擎、解码 worker、库句柄
  src/library.rs     // 索引生命周期、后台索引线程
  src/error.rs       // AppError → 字符串，跨 IPC 边界
  tauri.conf.json    // 窗口、asset 协议、CSP
  capabilities/      // ACL：core:default + drag:default
  tests/ipc.rs       // 不打开窗口就能跑完的验收
ui/                  // SolidJS + Vite，构建产物被编进 exe
tools/               // run-app / kill-app / CDP 驱动脚本
```

## 三层，各自的规则

**命令层不决定任何事。** `commands.rs` 里的每个 `#[tauri::command]` 只做三件事：校验参数形状、调用 `AppState` 的方法、返回线上类型。逻辑放在 `state.rs`，因为这一层能用 `cargo test` 直接驱动——`tests/ipc.rs` 里的 15 个测试全部走真实的 `AppState`，不经过窗口。只有靠人点按钮才能验证的外壳等于没验证。

**引擎只有一个实例，进程级常驻。** WASAPI 流在 `setup` 里打开，直到进程退出。这跟 P1 的取舍一致：共享模式的 12ms 排队是打开流就付掉的，反复开关只会更慢。设备打不开时应用照常启动，`get_status` 报 `audioAvailable: false` 与引擎自己的错误文本（带设备名或格式），播放命令返回同一条错误——一个坏设备不该让窗口起不来。

**解码在专用 worker 上，队列是"后到者赢"。** 3 分钟 96kHz 文件解码 112ms，跑在命令线程上会把 webview 卡住。`LatestSlot` 是一个只存一件活的信箱：新请求替换排队中的旧请求，被替换的那个收到 `superseded`。按住方向键扫过一个列表时，只解码最终停下的那一行，而不是路过的每一行。

**播放头是纯原子读。** `get_playback_position` 只做几个 `AtomicU64` 载入，不取 `arc-swap` guard、不加锁、不分配——它每秒被调 60 次。`active_frames` 是跟 clip 一起发布的，所以时长也不需要碰槽位。

## 从 P1 到 P3 的引擎改动

P1 的 `Engine` 是给命令行用的，P3 需要三处变化，理由都在 IPC 的用法里：

1. **`publish` / `publish_from` / `stop` 改成 `&self`。** 它们只写原子量与 `arc-swap`，不需要 `&mut`。改完之后任何线程都能直接发布，命令层不必再搭一个控制线程来串行化。
2. **`publish_from(clip, offset_frames)`。** IPC 的 `start_at` 落到这里。seek 是"在不可变缓冲上换起始位置"，代价与普通播放相同的一次原子写，不重新解码、不裁剪。
3. **`PlaybackMonitor`。** `Engine::monitor()` 克隆一份 `Arc<Shared>` 加格式信息，给 webview 轮询用。`drain_reports()` 也在上面，所以状态栏能显示**实测**的 `publish → 出声`，而不是文档里的目标值。

波形加了 `reslip-engine::waveform`：`Decoder::decode_source` 把文件解成原始采样率与声道数的交错 f32（不做重采样、不做声道映射、不打包字节），`bucket_peaks` 再把它压成 1000 个峰值。

**桶边界是精确的，不是按 256 帧块量化的。** 参考实现先按 256 帧取块最大值再合并到桶，桶边界被量化到 256 帧；在 2 秒文件的 1000 桶视图上，这意味着每个桶的边界最多偏 5.8ms，一个尖锐的瞬态会画到隔壁的柱子上。现在的实现直接按 `[b*frames/buckets, (b+1)*frames/buckets)` 遍历每一帧。

**峰值来自播放用的同一次解码**，包括 gapless 裁剪。否则波形会和声音差 23–25ms——正好是一个瞬态的位置。

## IPC 表面

| 命令 | 返回 | 说明 |
| --- | --- | --- |
| `get_status` | `AppStatus` | 设备、格式、引擎周期、库状态，一次往返 |
| `play(path, startAt?)` | `()` | 发布音频；同一文件重播走缓存 |
| `stop` | `()` | 清空两个槽，停流 |
| `get_playback_position` | `PlaybackPosition` | 原子读，给 rAF 轮询 |
| `get_waveform(path, buckets?)` | `WaveformDto` | ~1000 个 f32，附带解码耗时 |
| `open_library(root)` | `LibraryStatus` | 返回时库可读，索引在后台跑 |
| `get_library` | `LibraryStatus?` | 索引进度轮询 |
| `search(request)` | `SoundDto[]` | 同 `reslip-index` 的过滤条件 |
| `list_folders` | `FolderNodeDto` | 从索引构建的树，不读 `readdir` |

线上字段一律 camelCase（`#[serde(rename_all = "camelCase")]`），前端类型手写在 `ui/src/ipc.ts`，与 Rust 侧一一对应。

## 验证

**Rust 侧 22 个测试，不需要窗口：**

- `tests/ipc.rs` 15 个，跑在真实设备上（本机 FxSound；`RESLIP_TEST_DEVICE` 可换），覆盖：状态报的是真实设备格式而非默认值、设备缺失时报引擎原文、播放推进播放头、`startAt` 偏移生效、同文件重播不重新解码、被取代的播放不播旧音频、非音频路径在解码前拒绝、缺文件报错不挂起、波形是整段 1000 点且响/静两半可分辨、桶数上下限被夹住、打开库后台索引后搜索命中、文件夹树来自索引且空文件夹不出现、无库时错误可显示、重复打开同一库不启第二个索引器、不存在的文件夹被拒。
- `commands.rs` 7 个，覆盖时长/声道标签格式、排序白名单（未知列名报错而不是拼进 SQL）、分页上限钳制。

**真实 28 万文件库（`D:\reslip-test-library`，本机 E2x2 之外的默认设备 FxSound）：**

| 口径 | 实测 |
| --- | --- |
| `open_library` → 首屏有行 | **69 ms** |
| 搜索 `gunshot` | 38 行，约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| 增量索引全程 | **6.5 s**（与 P2 的 5.4s 同量级，差异是这里走 `latest` 顺序） |

索引期间 UI 仍可查询（WAL），数字在侧栏与状态栏实时增长。

**交互用 CDP 驱动真实窗口验证，不靠人看：** `tools/check-interactions.mjs` 断言播放头推进（0.61→1.12s）、点波形 75% 处 seek 到 94.68s（目标 94.06s）、停止后播放头钉住不动。

## 踩到的坑

**`tauri` 必须开 `custom-protocol`。** 不开的时候 release 构建仍然把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是一片空白窗口——第一次运行正是这样，页面 URL 是 `http://localhost:1420/` 而不是 `http://tauri.localhost/`。开了之后 asset 协议从 exe 内部取文件。

**Windows 不允许替换正在映射的 exe。** `cargo build --release` 会在 `failed to remove file reslip.exe` 处失败。`tools/kill-app.sh` 用 PowerShell 的 `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

**想让 UI 可验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。`tools/inspect-app.mjs` 截图并抓控制台错误，`tools/drive-app.mjs` 驱动打开库/点行/搜索。这些脚本放在 `slip-sound/` 下跑，因为 `playwright-core` 装在那里，而 ESM 的模块解析按脚本位置找 `node_modules`。

## 已知未做

- 打开库用的是 `window.prompt`，不是原生文件夹对话框。原生对话框要在 P4 连同 UI 一起做（需要 `tauri-plugin-dialog` 与对应 ACL）。
- 分类侧栏只有文件夹树，没有 category/subcategory 的 facet 计数与收藏行。`reslip-index` 已经有 `facets()`，缺的是 P4 的侧栏交互规格。
- 拖拽导出（P0 已验证）还没接到列表上。
- 键盘操作（方向键上下选、空格播放）属于 P4。
- 长文件仍整段解码（P1 取舍）；波形因此也是一次整段解码，`decodeMs` 直接显示给用户。
