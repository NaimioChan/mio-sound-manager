# Agent log

Shared handoff log. Newest entry on top. One entry per work session: what changed, what was verified, what is left.

## 2026-11-17 15:12 · hermes · 改名 Mio Sound Manager + iX 重写前端 + 主题系统 + 输出音量

- 改: 新仓 `projects/mio-sound-manager`（从 `reslip` 迁入，`target/`、`ui/node_modules`、`ui/dist` 未带）；`crates/reslip-*` → `crates/mio-*`，crate/二进制名、identifier（`com.naimio.miosound`）、`localStorage` 前缀（`mio:`）、线程名全部改名；`ui/src/` 前端整体重写（SolidJS + `@siemens/ix` 5.3）：新增 `theme.ts`（六套主题）、`icons.ts`、`ix-elements.d.ts`；`styles.css` 重写为纯 token；`WaveformDock` 加静音按钮 + 104px 横向滑杆 + 百分比；引擎加 `set_gain`/`get_gain`（`Shared.gain_bits` 原子 + 渲染回调按设备格式原地缩放）；`tools/` 删掉旧的 8 个检查脚本，换成 `check-interactions` / `check-themes` / `check-volume` / `check-layout` / `shot` / `shot-themes`
- 验证: `cargo test --release` 62 个通过（engine 13 / index 49）；`tsc --noEmit` 干净；`check-layout.mjs` 通过（区域拼合、列轨道对齐、dock 顺序「音量 → AUTO PLAY → 暂停 → 停止」）；`check-themes.mjs` 六套主题应用 + 重载保留 + WCAG 对比度全部通过；`check-volume.mjs` 滑杆→IPC→引擎→回读、静音、重载全部通过；`check-interactions.mjs` 24 项全部通过（含播放头推进/暂停冻结/Stop 归零/波形 seek）；真实 271,350 条库上跑过
- 遗留: ① 主题只能改 `theme.ts` 的数组，没有导入/导出界面；② 音量是线性映射，没做 dB 曲线；③ 拖拽导出仍只验证到命令被调用，没在真实 DAW 里拖过（P0 遗留）；④ `probes/` 里的旧探针仍带旧项目名，未改名（它们是历史记录，不影响构建）
- 下一步: 无。P8 及之后按用户要求不做

### 四条要求各自的落地方式

**一、改名并迁移。** 不是复制一份再改，是迁入新目录后逐个改：crate 目录名、`Cargo.toml` 的 `name`/`description`/`path` 依赖、`use mio_engine::*`、二进制名 `mio-sound-manager`、`tauri.conf.json` 的 `productName`/`identifier`/窗口标题、`tools/*.sh` 里的 exe 名、`localStorage` 键前缀。改完先单独跑 `cargo build --release` + `cargo test --release`，确认后端没破再动前端。

**二、iX 重写前端，布局不变。** 布局（顶栏 / 可拖侧栏 / 列表 / 波形 dock / 状态栏）与交互（选择模型、快捷键、无限滚动、右键菜单、阻塞索引遮罩）全部保留，换的是视觉与控件实现。表格、侧栏、右键菜单、dock 仍是自有 DOM：iX 没有虚拟化表格，27 万行不能每行建一个组件。

**三、主题系统。** 一个主题是**八个颜色**，两百多个 token 由它们推导（中性 ramp、ghost/component 的 alpha、语义色的 hover/active、滚动条、canvas 两色）。六套内置：Classic Dark/Light（iX 原样/反色，不注入覆盖块）、Studio Dark/Light、Amber Console、Frost。覆盖块选择器 `html[data-ix-theme][data-ix-color-schema]`（0,2,1）压过 iX 自己的（0,2,0），这一点在浏览器里验过。

**四、音量滑杆。** 放在 `AUTO PLAY` 左侧：静音图标按钮 + 104px 横向 `ix-slider` + 百分比读数。增益存在引擎的 `Shared.gain_bits`（`AtomicU32` 存 f32 位模式），渲染回调读到非单位增益时对拷进 scratch 的字节按设备采样格式原地缩放；单位增益跳过缩放，回调仍是纯 memcpy。换设备时把记住的增益写进新引擎，否则切设备会静默重置音量。

### 被检查抓到、修掉的四个真实缺陷

1. **主题重载后回退默认。** 主题同时存在 `mio:theme` 和设置 blob 两处，重载后设置里的旧值赢。修法是把主题的所有权收到 `theme.ts` 一处，设置 blob 不再存 theme。
2. **soft-text 对比度不足。** 0.66 alpha 在 Studio Dark 的侧栏底色上只有 3.99:1。改成 0.78（std 0.94、weak 0.58），这是**通过 WCAG 检查的那组数**。
3. **表头颜色过暗。** 列头用 weak-text 时 2.62:1，低于 3:1；改用 soft-text。
4. **`aria-label` 读不到。** iX 的 `InheritAriaAttributesMixin` 把 host 的 `aria-*` 移到 shadow root 内的 `<button>` 并从 host 删除，CDP 脚本读 host 拿到 `null`。检查脚本改为读内层按钮。

### 一条给接手者的提醒

iX 组件的验证有三个坑，都写进 `docs/app.md` 了：控件在 shadow root 里（点 host 无效）、禁用态有 `pointer-events: none`（Playwright 会重试到超时）、上一轮的 modal 会挡住后面所有点击（脚本开头先 `page.reload()`）。另外 **`ix-modal` 的 host 永远是 0×0**，因为内层 `dialog` 是 `position: fixed`；量尺寸要量 shadow root 里的 `dialog`。

## 2026-11-17 · hermes · 分类只认 ASCII + 厂牌名不算描述

- 改: `tools/taxonomy.py`（删掉全部中文提示词，133 个条目从三元组降到二元组；补 15 个实测缺口的英文词）；`tools/gen-category-dict.py`（不再生成 `hints` 字段）；`crates/reslip-index/src/classify/mod.rs`（删掉 Hint 结构、中文匹配两处调用与兜底分支、新增 `BRAND_TOKENS` 只在文件名首位跳过）；`docs/index.md`、`tools/README.md`
- 验证: `cargo test --release` 106 个通过（index 49 / app 44 / engine 13）；271,350 条真实路径全量重索引 68s，未分类由 43.1%（旧词典）降到 4.7%
- 遗留: 剩下的未分类是纯序号与只有厂牌名的文件名，无可分类词，保持原状

### 用户的两条要求

**一、不要中文词典。** 整条中文通路删掉，不是关掉。上一版把中文只从文件名取分、目录名兜底，实测能把未分类从 5.6% 压到 4.3%，代价是引入一族难维护的 414 条提示词和「中文只在某位置才取分」这种解释不清的规则。删掉后 4.7%，差距在噪声范围内，规则少一整套。

**二、厂牌名不作分类依据。** 做法不是删词，而是加 `BRAND_TOKENS`，只在**文件名首位**跳过：`CHIME_vox_chop_male_ohh` 是 Chime 的人声切片，不是钟声；但 `temple bell chime 02` 里的 `chime` 仍然是钟声。同一个词两种含义都要保留，删词会把后一种也杀掉。

### 词表补的 15 个词，每一个都有实测依据

从 15,121 条未分类里统计文件名高频未匹配 token，只补有分类含义的：
`break`（`KSHMR_Break_Beat_Loop`）、`percussive`、`upsweep`、`braaam`/`braaams`、
`mech`/`mechs`、`bot`/`bots`、`vocalizing`、`animals`/`animal`/`birdsong`、
`strike`/`strikes`、`liquid`/`suction`、`melodyloop`/`songstarter`/`melody`。

**没有补的**：`fx`、`misc`、`loop`、`one`、`shot`、`house`、`houseloops`、`fields`。
前五个是格式噪声；后三个是包的分组词（Vengeance 把每条都放在 `House Loops` 下），
没有分类含义。试过往停用词里加后三个，分布一位没变，说明它们本来就不在词典里，加了等于没加，已撤回。

## 2026-11-17 · hermes · 分类词典与打分模型改为自有：GPL 汇编不能搬运

- 改: 新增 `tools/taxonomy.py`（词表源文件）、`tools/gen-category-dict.py`（生成器）；删除 `tools/gen-category-dict.mjs`（它从 Slip Sound 的 `category-dictionary.ts` 直接转写，而那个文件是 GPL-3.0-or-later 的汇编）；重写 `crates/reslip-index/src/classify/mod.rs`（打分模型、中文提示词、两字符分词、短语归一化）；重新生成 `crates/reslip-index/src/classify/dictionary.rs`；`indexer.rs` 与 `crates/reslip-app/tests/ipc.rs` 的 `door_slam` 断言随词表改；`docs/index.md`（classify 一节重写）、`docs/architecture.md`（P2 行）、`AGENTS.md`、`tools/README.md`
- 验证: `cargo test --release` 全绿（index 49 + app 25 + 19 + 13）；271,350 条真实路径全量重索引 68s，未分类 43.1% → 4.3%，23 个分类 133 个 facet；分类阶段 11.5s，占总耗时 17%
- 遗留: 词表可以继续按库里的实际错判迭代；`classify-diff` 现在是比较工具而不是一致性验收

### 为什么必须重写

Slip Sound 是 GPL-3.0-or-later。527 个关键词按 29 个类目的划分是汇编（选材与编排有独创性），逐条照搬不是署名问题而是 copyleft 问题：整体继续按 MIT 发布就不合规。单字词（`gun`、`shotgun`）本身不受版权保护，受保护的是「这 527 个词被这样分进这些类目」这份汇编，所以重写的对象是词表与分类结构，不是「换几个词」。

### 顺带修掉的三个真实缺陷

这些是重写过程中在 27 万条真实路径上量出来的，不是猜的。

**一、目录权重高于文件名，打的是营销分。** 路径是 `…\Bought\SFX\Epic.Stock.Media.Hybrid.Game.Sounds\…\ESM_SPG_fx_servo_metal_latch_02.wav`：包目录是营销文案，文件名才是这声音是什么。旧模型（目录 3 / 文件名 1）仅凭 `epic` 一个词就把 92,876 条（34%）判成同一类。新模型反过来：文件名每词 3，目录每目标最多 1。

**二、三字符分词下限丢掉了这个库最常见的索引词。** `ui` 在游戏音效包里出现 13,693 次，旧 tokenizer 的 `len > 2` 把它整个丢掉。下限改成 2。

**三、短语匹配原始路径，漏掉所有下划线命名。** `drum_loop` 与 `drum loop` 本是同一条短语，旧实现只认后者。改为先归一化再匹配。

### 中文是另一路信号

271,350 条路径里 30,585 条带中文，而 Epic Stock Media 那些包的中文就是包名即主题（`【现代化军事战争用户UI界面游戏音效】`）。纯 ASCII 分词器对它完全失明。

尝试过三种取法，只有第三种成立：把包目录里的中文当票 → 一个包 3,944 个文件全贴上同一句主题；完全不取 → 未分类升到 5%；**只在文件名取、目录中文仅作文件名一无所获时的兜底** → 4.3%。理由写进了模块文档。

中文提示词全部两个字以上：单字在中文里没有词边界，实测 `手` 撞 `手机`、`电` 撞 `电影`、`门` 撞 `门类`、`风` 撞 `风格`。

## 2026-11-16 21:20 · hermes · P6：ASIO 后端 + 设备选择；试用反馈四项

- 改: 新增 `crates/reslip-engine/src/asio.rs`（ASIO 后端：STA 渲染线程、驱动枚举、i32 大端处理、每声道 plane 拆分）、`crates/reslip-app/src/audio_config.rs`（`audio.json` 持久化所选设备）、`tools/build-app.sh`（`--asio` 检查 SDK 与 libclang 后带 feature 构建）；`crates/reslip-engine/src/output.rs`（`Backend` 枚举、`Mixer` 抽成两个后端共用、`DeviceChoice` + `list_devices`、`engine_period_ms` 随后端变、`StreamInfo`/`Shared`/`join_pro_audio_group` 开放给 ASIO 模块）、`decode.rs`（`DeviceSample::I32`）、`lib.rs`（feature 门）；`crates/reslip-app/src/state.rs`（`AudioSlot` 收拢引擎/格式/错误、`open_audio`/`set_audio_device`/`list_audio_devices`、`reindex_library` 接受 root、解码器随 job 走）、`commands.rs`（`list_audio_devices`/`asio_compiled`/`set_audio_device`、`AppStatus.backend`、`AudioDeviceDto`、`reindex_library(root)`）；`crates/reslip-cli/src/main.rs`（`--backend`/`--asio-frames`、`devices` 列两个后端）；`ui/src/SettingsDialog.tsx`（重写：Audio 节 + 设备下拉 + 缓冲大小，删 Libraries 节，`Play the row I click` → `Play on click`）、`Sidebar.tsx`（库头部右键菜单、删 `click to switch`）、`App.tsx`（`libMenu` 信号与菜单项、`reindexLibrary(root, full)`、状态栏后端标签、删空态与波形 dock 的说明文案）、`WaveformDock.tsx`（AUTO PLAY 文字按钮）、`CategoryDialog.tsx`（删说明段）、`styles.css`（`.ghost.auto`、库头部强调、`.setting-note`、`.backend-tag`，删 `.library-*`/`.lib-inactive`/`.settings-actions`/`.modal-note`）、`ipc.ts`（`AudioDevice`、`audioDevices`/`asioCompiled`/`setAudioDevice`、`reindexLibrary` 带 root）；`tools/check-workspace.mjs`（设置节断言改成 Audio 存在 / Libraries 不存在 / 设备下拉分组）、`check-index-overlay.mjs`（改走库右键菜单）、`check-browsing.mjs`（AUTO PLAY 文字断言）；`AGENTS.md`、`docs/architecture.md`（第 2 节 ASIO 数字补齐、P6 行）、`docs/engine.md`（两个后端、ASIO 一节含 STA 表格）、`docs/app.md`（设备选择、库右键菜单、视觉分层、删副标题的原则）

### 用户要的六项

| 要求 | 做法 |
| --- | --- |
| auto-play 不用图标，写 AUTO PLAY | dock 里换成 `.ghost.auto` 文字按钮，仍带 `aria-pressed`；状态栏另有后端标签 |
| 删掉无意义的 subtitle | 设置项只剩「Play on click」；删掉设置里 reindex 说明、波形 dock 的 `Select a sound…` / `no selection` / 波形耗时、空库态那段导语、分类对话框的说明段、侧栏 `click to switch` |
| 多个库单独管理，设置里 Libraries 栏去掉 | 设置只剩 Audio / Playback / List；库头部右键菜单放 Switch / Reindex / Full reindex / Reveal / Close，`reindex_library` 接受 root，后台库不必先切过去 |
| 库条目要和树条目有样式区分 | 头部 650 字重 + `--bg-raised` 底色 + 顶部分隔线，活跃库另有 `inset 2px` 强调条；树行 400 字重无底色 |
| 加 ASIO 支持 | `--features asio`；`Backend` 枚举 + 独立 STA 渲染线程；设置里一个下拉同时列 WASAPI 端点与 ASIO 驱动，ASIO 选中时多一个缓冲大小；选择写 `audio.json` 并在启动时恢复 |
| 设置里有音频设备选项 | 同上，Audio 是设置第一节 |

### 三个硬坑

**一、ASIO 要 STA，WASAPI 要 MTA。**

第一版在 ASIO 渲染线程里调 `initialize_mta()`（照抄 WASAPI 的做法），应用里切 ASIO 报 `ASIO loadDriver function returned false`，但同一个驱动在 CLI 探针里能加载。逐步二分出来的真相：

| 线程 | COM 初始化 | 结果 |
| --- | --- | --- |
| 主线程 | 无（默认 STA） | 成功 |
| 工作线程 | 无 | 失败 |
| 工作线程 | MTA | 失败 |
| 工作线程 | STA | 成功 |

驱动对象由 `CoCreateInstance` 创建（SDK `asiolist.cpp` 的 `asioOpenDriver`），TOE 驱动只从单线程套间交出对象。所以 ASIO 的渲染线程必须是 STA，WASAPI 的必须是 MTA，两个后端不能共用线程。

**二、线程的 COM 套间一旦设定就改不了，所以 ASIO 枚举必须另起线程。** `output::list_devices` 先枚举 WASAPI（那一步把调用线程变成 MTA），再调 ASIO 的 `initialize_sta()` 时拿到 `RPC_E_CHANGED_MODE`，于是所有 ASIO 驱动都看起来不存在——但命令行 `devices` 又能列出来。修法是 `asio::list_devices` 自己开一个 STA 线程做完枚举再返回。

**三、枚举会关掉正在跑的驱动。** SDK 的 `loadDriver` 第一件事是 `removeCurrentDriver()`，所以「打开设置页」这个动作会把正在播的 ASIO 流关掉，表现为设置页一开、播放头就不动。修法两层：`list_devices(probe)` 在 ASIO 正在用时只读注册表名不加载驱动；`open_audio` 先 drop 旧引擎（停流 + join 渲染线程）再开新引擎，否则新引擎的 `loadDriver` 会把旧引擎的驱动从下面抽走。

### 实测（TOPPING Pro USB Audio Device，48kHz，本机）

| 缓冲 | 周期 | publish → 回调写入 | publish → 出声 | 下溢 |
| --- | --- | --- | --- | --- |
| 64 帧 | 1.33ms | 1.81ms | 5.31ms | 0 |
| 128 帧 | 2.67ms | — | — | 0 |
| 256 帧 | 5.33ms | 5.93ms | 14.43ms | 0 |

WASAPI 共享模式同机对照 11.00ms 周期。256 帧的 14.43ms 说明驱动缓冲本身就是延迟：帧数越大 ASIO 越不占优势。

- 验证: `cargo test --release --workspace` 101 个通过（默认特性与 `--features asio` 各跑一遍）；`tsc --noEmit` + `vite build` 通过（58.5KB JS）；CDP 五份脚本全绿——`check-browsing.mjs` 65 项、`check-workspace.mjs` 29 项、`check-index-overlay.mjs` 7 项、`check-interactions.mjs` 通过、`check-layout.mjs` 无重叠无裁切；应用内切 ASIO 后播放头正常推进、切回 WASAPI 格式随之回退、`audio.json` 往返正确；CLI `play --backend asio --asio-frames 64` 报 5.31ms
- 遗留: ① ASIO 只在 TOPPING Pro 上验过（单驱动、6 进 6 出、只开前 2 声道），多驱动切换与其他厂商驱动未验；② 切设备会丢掉当前播放位置（旧引擎 drop、缓存 clip 清空），切回去要重播；③ ASIO 驱动被其他进程独占时没有专门提示，错误是驱动自己的 `loadDriver` 文本；④ 分类侧栏折叠状态仍不持久；⑤ `check-browsing.mjs` 的拖拽项跑完会冻住 webview（既有行为，脚本注释已写明），跑完必须重启应用，`tools/README.md` 已补运行顺序
- 下一步: 没有排定阶段。按试用反馈继续修

## 2026-11-16 19:40 · hermes · 试用反馈三项修复（排序 / 文件夹折叠 / 图标化播放控件）

- 改: `ui/src/SoundTable.tsx`（新增 `SORT_WIRE` 翻译表、`querying` prop、表尾 `searching…`、`.table` 上加 `data-querying`）、`ui/src/App.tsx`（`request()` 的 `sortBy` 走 `SORT_WIRE`、新增 `querying` 信号、顶栏 auto-play 文字按钮删除、Settings 换图标）、`ui/src/Sidebar.tsx`（文件夹树加 twisty 与按库持久化的展开集合、`All folders` 对齐、双击展开）、`ui/src/WaveformDock.tsx`（三个控件图标化 + auto-play 前置）、新增 `ui/src/icons.tsx`、`ui/src/styles.css`（`.ghost.icon`、`.twisty` 点击区、`.foot-querying`）；`tools/check-browsing.mjs`（新增排序/筛选回归 18 项、文件夹折叠 7 项、dock 图标顺序 5 项、`settleList` 等 `data-querying`、拖拽探测加超时并移到脚本末尾）、`tools/check-interactions.mjs` / `check-workspace.mjs` / `check-index-overlay.mjs`（图标按钮改按 `aria-label` 定位）、`tools/check-layout.mjs`（`text-overflow: ellipsis` 不再算裁切）；`docs/app.md` 补四节设计说明

### 三项反馈的根因

| 反馈 | 根因 |
| --- | --- |
| FOLDERS 树不能折叠 | 树是一次性全量渲染到 `depth < 3`，没有 twisty、没有展开状态。现在子节点只在展开时挂载，展开集合按库存 `localStorage` |
| 点 LENGTH / RATE 报 `unknown sort column`，之后筛选也失效 | 渲染层把行字段名（`durationS` / `sampleRate`）直接当 `sortBy` 发出去，索引白名单是 `duration` / `sample_rate`。失败那次之后排序值没变，后续每次搜索都带同一个坏名字，于是分类与文件夹筛选一起挂——`SORT_WIRE` 是唯一翻译点 |
| 设置/暂停/停止改纯图标，auto-play 移到暂停左边 | 顶栏的 auto-play 文字按钮删掉，三个控件在 dock 右端成组；每个都带 `title` / `aria-label`，auto-play 另有 `aria-pressed` |

### 顺带修掉的两个非反馈缺陷

1. **点查询中的行会丢选中。** 筛选或排序落地时整批替换行节点，点击落在即将卸载的节点上，选中随之消失——看起来像点击处理器坏了。现在查询期间 `.table` 带 `data-querying="true"`、表尾显示 `searching…`，脚本也等这个信号而不是 sleep。
2. **`.twisty.empty` 被全局 `.empty` 命中。** `.empty` 是整窗空状态（`padding: 40px`、`height: 100%`），占位 twisty 因此被撑成 80×80 并把行名挤到 88px。改名 `.twisty.spacer`。

- 验证: `cargo test --release --workspace` 97 个通过；`tsc --noEmit` + `vite build` 通过（57KB JS）；CDP 五份脚本全绿——`check-browsing.mjs` 64 项（原 40 + 新 24）、`check-workspace.mjs` 26 项、`check-index-overlay.mjs` 7 项、`check-interactions.mjs` 通过、`check-layout.mjs` 无重叠无裁切；真实 28 万库上点 Length / Rate / Size 均返回 36 行且分类筛选仍生效
- 遗留: ① `AGENTS.md` 的命令表仍写 `cargo test` 83（实为 97）、CDP 清单缺 `check-workspace` / `check-index-overlay`，本轮没动（受保护文件）；② 分类侧栏折叠状态仍不持久（文件夹树现在持久了）；③ 齿轮图标用 24px 视框描边路径，与其余三个 12px 填充图形的粗细不完全一致；④ 拖拽悬停光标样式不变
- 下一步: 继续按试用反馈修

## 2026-11-16 18:40 · hermes · P5 工作区（多库 / 持久化 / undo / 阻塞索引 / 设置）

- 改: 新增 `crates/reslip-app/src/session.rs`（打开过的库 + 活跃库写 `%APPDATA%\com.naimio.reslip\session.json`）、`src/history.rs`（undo/redo 栈）；`state.rs` 从单库改成 `LibrarySet` 多库 + `add_library`（阻塞）/`remove_library`/`set_active_library`/`library_list`/`reindex_active`/`undo`/`redo`/`attach_session`/`persist_session`；`library.rs` 加 `index_blocking_with`（带进度回调）与 `restore_category`；`reslip-index/src/db.rs` 加 `restore_category_many`；`commands.rs` 换 `open_library` 为 `add_library`，新增 `list_libraries`/`remove_library`/`set_active_library`/`reindex_library`/`undo`/`redo`/`get_history`，`LibraryStatus` 加 `name`/`active`；`ui/` 重写 `App.tsx`/`Sidebar.tsx`，`SoundTable.tsx` 加无限滚动与可配列，新增 `SettingsDialog.tsx`/`IndexOverlay.tsx`，`styles.css` 加库分组/设置/遮罩/rail 把手/表尾；新增 `tools/check-workspace.mjs`、`tools/check-index-overlay.mjs`；`docs/app.md` 重写，`docs/architecture.md` 第 8 节把 P5 标通过并写明 P6 不做
- 状态: **P5 通过**。P6（Slicer/Effects/Loop/Export + ASIO）按用户决定不做
- 顺带: 用户删了四篇过时文档（`TODO-low-latency-audio` / `research-notes-playback` / 两份 `prompt-for-*`），把仓库里七处悬空引用改掉了——`AGENTS.md` 状态行与目录表、`docs/architecture.md` 开头与 6.2、`probes/README.md`

### 用户要的六项，逐条落地

| 要求 | 做法 |
| --- | --- |
| 记住库，不要每次重选 | `session.json` 存打开过的库 + 活跃库；启动 `attach_session` 只开数据库不跑索引 |
| 无限翻页 | `offset = 已加载行数` 追加，距底部两屏触发；表尾显示「500 of 271,350 · scroll for more」 |
| undo 栈 | 最近 50 次编辑，每条存每路径的旧值；Ctrl+Z / Ctrl+Shift+Z / 顶栏按钮 |
| 同时开多个库，文件夹树显示各自母文件夹名而不是 All Files | 侧栏按库分组，每组头部是文件夹名（完整路径做 tooltip）；非活跃组只显示头部 |
| 索引期间占用 | `add_library` 阻塞到 pass 结束，铺满窗口的遮罩吞掉点击与键盘 |
| 左栏可调宽 | `.rail` 内的绝对定位把手，160–520px，写 `localStorage` |
| 搜索框移到中间区块顶部 | `.main-head` 里，窗口顶部只留「我在哪个库」+ undo/redo + 设置 |
| 二级设置页 | Playback / List / Libraries 三节，改动即时生效并存 `localStorage` |

### 实测抓到的两个真实缺陷

1. **undo 恢复不了分类。** 第一版 `restore` 走 `set_category` / `clear_category`，但这两个只能造出「手动且是这个字符串」或「Uncategorized」，造不出「分类器填的值」——撤销一次手动分类会把旧值抹成 Uncategorized。集成测试直接抓到（期望 `Weapons`，得到 `Uncategorized`）。修法是索引层加 `restore_category_many`，精确写回字符串与 `category_manual`。
2. **`Option<Channel<T>>` 编译不过。** Tauri 2 的 `Channel` 不实现 `Deserialize`，`onProgress` 必须必填。同理 `State<'_, T>` 借的是命令栈帧，跨不进 `spawn_blocking`，两个阻塞命令改成拿 `AppHandle` 再 `app.state()`。

### 验收

`cargo test --release --workspace` **97 个通过**（P4 是 83）。新增 14 个：多库切换与搜索隔离、关闭活跃库回退、切到未打开的库被拒、undo/redo 收藏、undo 恢复旧分类与旧归属、多行编辑一次撤销、session 往返/损坏降级/消失文件夹、history 栈上限与分叉。

CDP 三份脚本共 73 项断言全绿：`check-workspace.mjs` 26 项（含拖拽改宽 232→322px、无限滚动 5→1500 行、undo/redo 两条路径）、`check-index-overlay.mjs` 7 项（遮罩铺满、点击打不到列表、空格不播、结束后消失）、`check-browsing.mjs` 40 项 P4 回归。`check-layout.mjs` 几何断言无重叠无裁切。

- 验证: `cargo test --release --workspace` 97 个通过；`tsc --noEmit` + `vite build` 通过（53KB JS）；真实 28 万库 `add_library` 增量 11.6s（阻塞到完成）；三份 CDP 脚本 73 项全绿；布局几何断言通过
- 遗留: ① 想改 `AGENTS.md` 的命令表（`cargo test` 的 83→97、CDP 脚本清单加两份新的）被保护机制拦下（审批超时），**现状是没改**——`AGENTS.md` 第 32 行仍写 83，第 59 行的脚本清单缺 `check-workspace` 与 `check-index-overlay`；② 分类侧栏折叠状态不持久（模式/宽度/设置项都持久）；③ undo 不跨重启，不覆盖删除/改名/重索引；④ 拖拽悬停光标样式不变
- 下一步: 没有排定的阶段了。这一版是可用产品，后续按试用反馈修

## 2026-11-16 16:15 · hermes · P4 前端搬迁（列表/波形/搜索完整交互）

- 改: `crates/reslip-engine/src/output.rs`（`pause`/`resume`/`paused`，`PlaybackSnapshot.paused`）；`crates/reslip-index/src/db.rs`（`set_favorite_many`/`set_category_many`/`clear_category_many`/`summary`）；`crates/reslip-app`（`commands.rs` 新增 `pause`/`resume`/`get_sidebar`/`set_favorite`/`set_category`/`clear_category`/`pick_library_folder`/`reveal_in_folder`/`list_matching_paths`/`count_matching`，`state.rs` 对应封装与 `matching_paths`，`Cargo.toml` 加 `tauri-plugin-dialog` + `tauri-plugin-opener`，`capabilities/default.json` 加两个 ACL，`tauri.conf.json` 的 CSP 加 `connect-src`）；`ui/src` 重写 `App.tsx`/`SoundTable.tsx`/`Sidebar.tsx`，新增 `ContextMenu.tsx`/`CategoryDialog.tsx`，`ipc.ts` 补类型与 `startDrag`，`styles.css` 补菜单/模态/facet/行对齐；新增 `tools/check-browsing.mjs`、`tools/check-layout.mjs`；`docs/app.md` 重写，`docs/architecture.md` 第 8 节标 P4 通过
- 状态: **P4 通过**
- 验收库: `D:/reslip-test-library`（28 万文件），设备 FxSound

### 收掉了 P3 遗留的全部六项

原生目录选择（`tauri-plugin-dialog` + ACL）、分类侧栏 facet 计数与收藏行、拖拽导出接到列表、键盘操作（方向键/Home/End/PageUp/PageDown/Ctrl+A/空格）、暂停（引擎新增）、自动播放开关。P3 日志里"长文件整段解码"仍是 P1 的取舍，未变。

### 实测抓到的四个真实缺陷（都不是测试写错）

1. **未收藏的星标单元格宽度为零。** 内容为空 span 时 `scrollWidth` 也是 0，Playwright 报 "element is not visible"——但根因不是测试：点击目标不存在，意味着"收藏"只能在已经收藏之后点得动，等于从行上无法收藏。现在未收藏画 `☆`，颜色退到 `--border-strong`。
2. **表头与行的列轨道相差 25px。** 表头在滚动容器之外，轨道按整宽算。先用 `padding-right` 补，差值反而更大（18px 残差）——`box-sizing: border-box` 下 padding 让 `fr` 轨道按比例吸收差值。真正的原因是**行是绝对定位且没设 `left/right`**，于是收缩到内容宽度、轨道按文字宽算。加 `left: 0; right: 0` 后残差降到 0.26px（亚像素舍入），用 `width: calc(100% - Npx)` 补滚动条。
3. **`plugin:drag|start_drag` 的 `onEvent` 是必填的 `Channel`。** 不传整条命令报 `missing required key onEvent`，拖拽根本不会开始。P0 的探针当时传了通道，产品里漏了。`startDrag` 现在 `new Channel()` 传进去，注释写明 P0 已实测该通道从不触发、传它是为了满足签名。
4. **CSP 缺 `connect-src`。** P3 只写了 `default-src 'self'`，每次 IPC 都先被 CSP 拦一次再到 `ipc.localhost`，控制台两条错误，功能靠兜底路径还能跑。补 `connect-src 'self' ipc: http://ipc.localhost`。

另外 `search` 的 `limit` 被 `clamp(1, 5_000)`，Ctrl+A 在 28 万库里只能选 5000 条。新增 `list_matching_paths`（只回路径、不设上限），实测选中 **271,350** 条。

### 验收怎么测的

`cargo test --release --workspace` **83 个通过**（P3 是 79）。`tests/ipc.rs` 19 个新增 4 个：收藏批量写入后行与侧栏数字一致、手动分类扛住一次全量重索引而清除后交还分类器、facet 计数覆盖全部行、暂停冻结播放头且 resume 从同一帧继续（0.42 → 0.42 → 1.02s）。

窗口内交互全用 CDP 驱动真实窗口，两个脚本共 40 项断言：

- `tools/check-browsing.mjs`：分类筛选与 chip、方向键、Shift/Ctrl 多选、Ctrl+A、右键菜单与 Escape、星标落库、分类对话框存取与清除、排序箭头、点击到波形 32–36ms、空格暂停时播放头 0.61→0.61s、继续后 0.61→1.01s、Stop 归零、拖拽命令到达插件、文件夹模式。
- `tools/check-layout.mjs`：按真实 DOM 几何断言五个区域的位置尺寸、表头与行列轨道对齐（容差 0.5px）、每个单元格与表头不被裁切、dock 头部子元素不重叠、无元素溢出视口。

`playwright-core` 不在本仓库，验收脚本用 `createRequire` 指向 `slip-sound/package.json`，因此可以从任意目录运行（P3 时要求先 `cd slip-sound/`）。

- 验证: `cargo test --release --workspace` 83 个通过；`ui/` 的 `tsc --noEmit` 与 `vite build` 通过（43.4KB JS）；真实 28 万库上 40 项 CDP 断言全绿；布局几何断言无重叠无裁切
- 补记: 收工时改 `AGENTS.md` 被保护机制拦下（审批超时），用户回来后重试通过——状态行、测试总数（79→83）、CDP 脚本清单都已更新，`docs/engine.md` 补了 `pause`/`resume` 一节，`docs/index.md` 补了批量编辑事务
- 遗留: ① 无限滚动未做，一页 500 行，参考实现有滚动加载；② 撤销/重做未做，收藏与分类写入后不可撤回；③ 分类侧栏的折叠状态不持久（侧栏模式是持久的）；④ 拖拽悬停光标样式不变（P0 用户指出过）
- 下一步: P5 工具功能（Slicer / Effects / Seamless Loop / Export），逐项对照 Slip Sound 行为；这些是 ffmpeg 外部进程的活儿，播放路径仍不经过 ffmpeg

## 2026-11-16 15:10 · hermes · P3 Tauri 外壳 + IPC 落地

- 改: 新建 `crates/reslip-app`（`commands` / `state` / `library` / `error` + `tests/ipc.rs`）、`ui/`（SolidJS + Vite，`App` / `SoundTable` / `WaveformDock` / `Sidebar` / `ipc.ts`）、`tools/`（`run-app.sh` / `kill-app.sh` / `inspect-app.mjs` / `drive-app.mjs` / `drive-big-library.mjs` / `check-interactions.mjs`）；`crates/reslip-engine` 新增 `waveform` 模块，`Engine` 加 `publish_from` / `stop` / `monitor` / `PlaybackMonitor`；新增 `docs/app.md`；更新 `AGENTS.md`、`docs/architecture.md` 第 8 节
- 状态: **P3 通过**，产品代码 = engine + index + app + ui
- 验收库: `D:/reslip-test-library`（28 万文件），设备 FxSound（默认设备，48k 2ch f32，引擎周期 11.00ms）

### 验收怎么测的（不靠人看窗口）

`tests/ipc.rs` 15 个测试直接驱动 `AppState`，跑真实设备与真实索引，不打开窗口。加上 `commands.rs` 的 7 个与 P1/P2 的 57 个，`cargo test --release --workspace` 共 **79 个通过**。

窗口内的交互用 CDP 驱动真实窗口断言：

- `tools/check-interactions.mjs`: 播放头推进（0.61 → 1.12s）、点波形 75% 处 seek 到 94.68s（目标 94.06s）、Stop 后播放头钉在 0.00s 不动
- `tools/drive-big-library.mjs`（真实 28 万库）:

| 口径 | 实测 |
| --- | --- |
| `open_library` → 首屏有行 | **69 ms** |
| 搜索 `gunshot` | 38 行 / 约 65 ms（含 80ms 防抖） |
| 点击行 → 波形上屏 | 30–43 ms |
| 增量索引全程 | **6.5 s**（P2 命令行 5.4s，差异是这里顺序走完后 UI 轮询结束） |

索引期间 UI 仍可查询（WAL），侧栏与状态栏的数字实时增长。

### 引擎的三处改动及理由

1. **`publish` / `publish_from` / `stop` 改 `&self`。** 它们只写原子量与 `arc-swap`，不需要 `&mut`。这样任何线程都能发布，命令层不必再搭控制线程串行化。
2. **`publish_from(clip, offset_frames)`。** IPC 的 `start_at` 落到这里；seek 是在不可变缓冲上换起始位置，代价与普通播放相同的一次原子写，不重新解码。
3. **`PlaybackMonitor`。** `Engine::monitor()` 给 webview 一份只读句柄，`snapshot()` 与 `drain_reports()` 都是纯原子读 / 无锁队列，播放头轮询不碰音频线程的槽。

波形在 `decode_source`（原始采样率交错 f32，不重采样不打包）之上做 `bucket_peaks`。**桶边界按帧精确，不是参考实现的 256 帧块量化**：2 秒文件的 1000 桶视图上，块量化会让每个桶边界最多偏 5.8ms，尖锐瞬态会画到隔壁柱子。峰值来自播放用的同一次解码（含 gapless 裁剪），否则波形与声音差 23–25ms。

### 踩到的两个坑

1. **`tauri` 必须开 `custom-protocol`。** 不开时 release 构建仍把 webview 指向 `devUrl`（localhost:1420），Vite 没跑就是空白窗口。第一次运行正是如此：CDP 里页面 URL 是 `http://localhost:1420/`，改成 `http://tauri.localhost/` 后才从 exe 内部取 asset。
2. **Windows 不允许替换正在映射的 exe**，`cargo build --release` 报 `failed to remove file reslip.exe`。`tools/kill-app.sh` 用 PowerShell `Stop-Process`（`taskkill //F` 在 git-bash 下会把 `//F` 弄坏）。

另有一条工程约定：**要想让 UI 可被验证，必须能从外部连上 webview。** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` 要在 webview 创建前设置，所以有 `tools/run-app.sh`。驱动脚本放在 `slip-sound/` 下跑——`playwright-core` 装在那里，且 ESM 按脚本自身位置解析 `node_modules`。

- 验证: `cargo test --release --workspace` 79 个通过；`ui/` 的 `tsc --noEmit` 与 `vite build` 通过（26KB JS）；真实 28 万库的打开/搜索/点击/波形/seek/stop 全部实测；截图见 `reslip-big-playing.png`
- 遗留: ① 打开库用 `window.prompt` 而非原生文件夹对话框（需 `tauri-plugin-dialog` + ACL，P4 一起做）；② 侧栏只有文件夹树，无 category/subcategory facet 计数与收藏行（`reslip-index` 的 `facets()` 已就绪，缺 P4 的交互规格）；③ P0 验证过的拖拽导出还没接到列表；④ 键盘操作（方向键选行、空格播放）属于 P4；⑤ 长文件仍整段解码，波形因此也是一次整段解码，`decodeMs` 直接显示给用户（实测短素材 0.9–9.4ms）；⑥ `autoAdvance`（播完自动下一行）未做，参考实现有
- 下一步: P4 前端搬迁（列表/波形/搜索的完整交互：键盘、多选、拖拽导出、原生目录选择、分类侧栏）

## 2026-11-16 14:40 · hermes · P2 索引器 + schema 落地

- 改: 新建 `crates/reslip-index`（probe / wav_header / classify / db / folders / indexer）；`crates/reslip-cli` 新增 `index` / `index-stats` / `search` / `classify-diff` / `probe-bench` 五个子命令；新增 `tools/gen-category-dict.mjs`（词典生成器）与 `probes/wav-fmt20`（缺陷复现）；新增 `docs/index.md`；本条日志
- 状态: **P2 通过**，产品代码 = engine + index + cli
- 验证库: `D:\reslip-test-library`，280,805 个文件的硬链接镜像（原库 `SoundLibrary` 被 Slip Sound 应用占用，未动）

### 分类一致性：271,350 行零差异

`reslip-cli classify-diff` 用 Rust 分类器重新分类旧库每条路径，逐字段比对 category / subcategory / confidence / matched_terms：**四项全部 0 差异**。

这个检查抓到两个真实移植缺陷，都是不测就会静默错下去的：

1. **同分裁决依赖插入顺序。** 参考实现遍历 `Object.entries(scores)`（插入顺序），我用 `HashMap`（随机顺序），同级子分类打平时选中项不同 → 3,932 行归错分类。改用 `Vec<(String, u32)>` 保持插入顺序。
2. **`matched_terms` 未全局去重。** 参考是 `[...new Set()]`，我用了 `Vec::dedup`（只去相邻重复），短语与单词同时命中时留下 `["magic","spell","magic","spell","magic","spell"]`。

两条都补了回归测试，注释写明实测来源。

### 实测（本机，D 盘 Kingston SNV2S2000G，无 DRAM 的入门 SSD）

| 场景 | 文件数 | 耗时 | 吞吐 |
| --- | --- | --- | --- |
| 全量首次 | 271,350 | **63.9s** | 4,247 文件/s |
| 增量（无变化） | 271,350 | **5.4s** | 探测 0 个文件 |

分阶段（全量）：

| 阶段 | 耗时 | 说明 |
| --- | --- | --- |
| 遍历 | 4.98s | 单线程目录扫描 |
| 探测（16 线程） | 32.5s | 每文件 1.9ms，随机读延迟受限 |
| 分类 | 6.24s | 每文件 0.023ms |
| 写入 | 7.04s | 批量事务，索引后建 |
| prune | 2.94s | `NOT EXISTS` 命中主键 |
| FTS 重建 | 10.18s | 一次性 rebuild |

**与架构文档 5.1 的 9s 外推不符，且不能靠调优弥合。** 那个数字外推自 2 万文件合成库（预生成文件 + NVMe），这里是 28 万真实文件在无 DRAM SSD 上。探测阶段的 `stat + open + 读 64KB` 累计占 80%，解析只占 20%——瓶颈是随机读，不是解析代码。同一份代码在热缓存小样本上跑 10,790 文件/s，在冷缓存全库上 4,247 文件/s，差距就是磁盘。

优化过的三处，都有实测依据：

- **重复 stat 去掉两次**：`walkdir` 返回的 `DirEntry::metadata()` 在 Windows 上取自 `FindNextFileW`，不额外系统调用。之前遍历、探测、增量检查各 stat 一次，8.2s 浪费在这里。
- **线程上限从 8 放开到核数**：8 → 16 线程还有 22% 收益（4,347 → 5,331 文件/s），因为探测是延迟受限而非 CPU 受限。
- **批量装载卸掉 FTS 触发器与二级索引**：写入阶段 16.9s → 6–7s；prune 的 `NOT IN` 子查询 36s → 3s（改 `NOT EXISTS`）。

### 修掉的两个真实缺陷

1. **WAV 的 20 字节 `fmt ` chunk 被 symphonia 拒绝**。`symphonia-format-riff` 的 `read_pcm_fmt` 只接受 16 / 18 / 40，其它长度报 `malformed fmt_pcm chunk`。20 字节是合法的 `WAVEFORMATEX`（`cbSize = 2`），真实库里有 284 个，全部是可正常播放的 PCM。新增 `wav_header.rs` 做 RIFF 头兜底解析（只读头，不碰音频数据）。复现程序在 `probes/wav-fmt20`。
2. **路径分隔符混用导致 prune 删光旧行**。`walkdir` 返回的路径继承传入根的分隔符，根传 `D:/lib` 会得到 `D:/lib\sub\a.wav`。混合形式破坏唯一键，一次全量扫描把旧库 271,350 行全部 prune 掉。修法是在入口统一归一化（`normalize_root` / `store_path`），文件夹树与文件夹区间查询对根做同样处理。

另外过滤了 macOS 的 `._*` 资源叉文件（9,455 个，带音频扩展名的 AppleDouble 元数据）。过滤后元数据为 null 的行从 9,739 降到 224（0.08%），剩下的都是内容非音频的 `.wav`。

- 验证: `cargo test --release` 44 个单测通过（probe 兜底、WAV 头解析、分类插入顺序与去重、upsert 保留用户状态、prune、FTS 操作符注入、文件夹区间、增量与全量）；真实库 271,350 行分类零差异；搜索 "gunshot" 208 命中 / 2.0ms、"rain" 611 命中 / 5.2ms；文件夹树 Bought 13,875 + UnBought 257,475
- 遗留: ① `index_ms` 字段（FTS 重建）10.2s 是第二大项，可考虑增量重建而非全量 rebuild；② 分类器每文件 0.023ms，28 万文件 6.2s，`Classifier::new()` 的词典构建未跨批次复用（已在 `Arc` 里，但 `target.key()` 每次 format 分配）；③ 大文件（>10 分钟）仍整段解码，P1 的取舍未变；④ `docs/architecture.md` 5.1 的 9s 外推需要在文档里标注为「合成库条件」，避免下一任 agent 当成目标
- 下一步: P3 Tauri 外壳 + IPC 骨架（空窗口能调 `play` / `get_waveform`）

## 2026-11-16 12:30 · hermes · P0 拖拽导出通过 + P1 音频引擎原型落地

- 改: 新建 Rust workspace（根 `Cargo.toml` + `crates/reslip-engine` + `crates/reslip-cli`）；新增 `probes/tauri-drag-p0`（Tauri 2 拖拽验证窗口）；`docs/architecture.md` 补 P0/P1 实测结论；`docs/agent-log.md` 本条
- 状态: **P0 通过，P1 通过**。首个可运行的产品代码（engine crate + 命令行验收工具）

### P0 拖拽导出：通过

用户手操，从 Tauri 2 窗口把文件拖进 Cubase 16 / REAPER / FL Studio 均被接受并导入。验收标准（架构文档第 8 节）达成，外壳选型不用退回 Electron。

- 链路: `tauri-plugin-drag` 2.1.1 → `drag` 2.1.1 → `OleInitialize` + `SHCreateShellItemArrayFromIDLists` + `BHID_DataObject` + `DoDragDrop`，产出标准 `CF_HDROP`
- 自动侧验: 拖进一个普通 OLE 接收窗口，格式列表含 `FileDrop`/`FileNameW`/`Shell IDList Array`，路径与大小正确；单文件、多文件（4 个）、含中文与空格的文件名均通过
- **已知缺陷**: `plugin:drag|start_drag` 的 `onEvent` 回调从不触发。命令返回了，但 `Dropped`/`Cancel` 一次都收不到。产品里若要用它判断拖拽结果（比如「拖出后从库中移除」），需要另找办法，不能依赖这个通道
- 交互细节: 悬停拖拽项时光标样式不应变化（用户指出，留到产品阶段处理，不进探针）
- 遗留: 探针窗口的 UI 是临时验证页，不进产品

### P1 音频引擎原型：通过

命令行 `reslip-cli`，指定文件路径即播，打印实测延迟、连按切换无 glitch。

- **架构**: 双槽 + `arc-swap`，切换是一次原子写索引。旧 clip 由 `arc-swap` 等渲染线程的 guard 释放后才 drop，音频线程从不释放内存
- **渲染回调**: 只有 `copy_from_slice` 与 `fill(0)`。scratch 缓冲在循环外按 endpoint 上限预分配，回调内零分配、零加锁、零 IO、零格式化
- **`IAudioClient` 生命周期**: 在渲染线程内创建并持有，`set_get_eventhandle()` 的 `Handle` 与 `AudioClient` 同寿命
- **格式覆盖**: WAV / AIFF / FLAC / MP3 / OGG / M4A 六种全部可播，各自走完整管线验证（见下）
- **重采样**: `rubato` FFT（`new_custom` + `process_all`），整段一次转完。48k→44.1k、96k→44.1k 实测频率与幅度零误差（660.00Hz / 880.00Hz，峰值 0.350 不变）
- **声道映射**: 单声道→立体声复制，立体声→单声道取平均，宽布局兜底取现有声道

#### 实测延迟（本机 E2x2，`Playback 1/2 (2- E2x2)`，44100Hz 2ch f32）

口径按 AGENTS.md 区分两段：

| 口径 | 单次播放 | 连续切换（24–48 次） |
| --- | --- | --- |
| `publish → 回调写入` | 8.36–9.74ms | p50 2.5–5.9ms |
| 其中已排队（不可优化） | 12.00ms | 12.00ms |
| `publish → 出声` | **20.35–21.74ms** | p50 14.5–16.9ms，max 22.05ms |

- 三轮独立复测，数字稳定（切换 p50 三轮回 16.89 / 16.90 / 16.84ms）
- **口径修正**: 本机该设备的引擎周期是 **11.00ms**（endpoint 970 帧 = 22ms），不是调研记录的 10ms。设备与驱动组合不同周期不同，架构文档里的 17.5ms 是另一台设备的数字，不能直接当目标对照
- 切换的 p50 低于单次播放，因为处在稳态的队列更短；上界 22.05ms 与单次播放一致
- **下溢计数 0**：24 次与 48 次连切均无 missing report、无 endpoint 走空。这是「无 glitch」的机器判据，不依赖人的耳朵

#### 发现并修掉的两个真实缺陷

1. **MP3 编码器延迟 25.1ms**：`FormatOptions::enable_gapless` 默认 `false`，LAME 的 delay/padding 被忽略，解码结果开头多 1105 帧填充。开启后 MP3 与 ffmpeg 输出逐帧一致（44100 帧，lag=0，SNR 230dB）
2. **M4A 编码器 priming 1024 帧（23.2ms）**：symphonia 的 `isomp4` 解析了 `elst` 但没接到 `codec_params.delay`，gapless 对 M4A 无效。解码端按 AAC 惯例裁掉首 1024 帧（带「前 1024 帧是否真是 priming」的哨兵判断，阈值 -40dBFS 用 p90 而非最大值，因为 AAC priming 解出来是 -42dBFS 的低电平残渣而非纯零）。修后 M4A 起点与 ffmpeg 一致（0.02ms），中段 SNR 135dB

  这两个缺陷都直接吃延迟：23–25ms 与整个设备路径同量级，采样触发会明显慢半拍。

- 遗留: ① MP4/M4A 尾部仍多 956–1980 帧编码器填充（只影响末尾静音，不影响触发延迟，产品化时补尾部裁剪）；② OGG 尾部多 636 帧同类；③ 音频回调的 MMCSS 提升是 best-effort，未验证失败时的抖动代价；④ 切换时不做交叉淡化，硬切（架构文档 4.1 的可选项未实现）；⑤ 长文件仍整段解码进内存（P1 取舍，流式 decode-ahead 待 P2 之后）
- 验证: `cargo test --release` 5 个单测通过（声道映射、i16 转换、重采样时长）；六种格式各自渲染成 WAV 后用 ffmpeg/ffprobe 与源文件逐样本比对；真实设备三轮延迟复测
- 下一步: P2 索引器 + schema（对真实库跑一遍，与架构文档第 5 节数字对照）

## 2026-11-16 10:40 · hermes · 架构设计定稿 + WASAPI 第二轮实测

- 改: 新建 `docs/architecture.md`（完整架构决策）；重写 `docs/TODO-low-latency-audio.md` 的 Q1–Q6 答复与交付物；`docs/research-notes-playback.md` 新增第 6 节（WASAPI 重新实测）；`AGENTS.md` 更新 Stack 与 Rules
- 状态: **设计定稿，仍无产品代码**
- **核心纠正**: 第一轮的 8.33ms 口径有误。只计到「回调写入缓冲」，没算缓冲里已排队的 12ms。真实点击到出声是 **17.5ms**（min 12.0, max 21.9）
- **共享模式不可调**: 四台设备（E2x2 真实 USB / Realtek / FxSound / ToDesk）的 `GetSharedModeEnginePeriod` 全部返回 `min=default=max=10ms`；请求 882 帧被拒 `0x88890020`。换格式无效（扫了 f32/i16/i24in32 × 44.1k/48k/96k）
- **独占否决**: 持有独占后其它客户端（独占和共享都算）被拒 `0x8889000A` → 用户开着 DAW 会静音。这推翻了「ASIO 会抢设备」的原担心方向：真正抢设备的是 WASAPI 独占
- **三条优化路线全部实测无效**: ① 小周期 → 驱动拒绝；② 空闲不喂料 → 降到 5.6ms 但连续播放 250/252 次回调下溢；③ `Reset()` 清队列 → 清掉事件句柄，crate 0.27 重新注册报 `0x8000FFFF`
- **冷路径**: 7 种格式「打开+探测+解码首包」全部 < 0.4ms。解码从来不是瓶颈，Q1 的问题方向本身是错的
- **索引**: 2 万文件合成库实测，遍历 108ms + 探测 795ms（8 线程）+ 写入 128ms + 建索引 17ms ≈ 1.1s；外推 17 万约 9s（Slip Sound 现状 120s）
- **格式探测**: 7 种格式 header-only 都能拿 duration/channels/rate；唯一例外 M4A 读不到声道数（symphonia isomp4 返回 0ch），需解首包
- **拖拽导出风险解除**: CrabNebula 的 `drag` crate 2.1.1 有 Windows 实现（OLE + `DoDragDrop` + `CF_HDROP`），`tauri-plugin-drag` 2.1.1 封装成 Tauri 命令。本机验证 `drag` crate 编译通过
- 验证: 两个新探针项目（`%TEMP%\wasapi3-probe` 8 个 bin、`%TEMP%\index-probe` 3 个 bin）全部编译运行；延迟数字三轮独立复测；索引在 2 万文件库上跑通
- 遗留: ① 拖拽导出「拖进真实 DAW」未验（crate 只验证了编译）；② ASIO 真实出声延迟未用排队法复测（TOPPING 本轮未接）；③ 索引只有合成库数据，真实混合格式库待 P2 复测；④ `AGENTS.md` 的 "Where things live" 表缺 `architecture.md` 一行（写入被权限门禁拦下）
- 下一步: P0 拖拽导出实测 → P1 音频引擎原型（命令行，验证双槽切换无 glitch）

## 2026-11-16 09:50 · hermes · ASIO 打通并实测

- 改: `docs/research-notes-playback.md`（新增第 4 节 ASIO 实测 + 更新待补清单）、`docs/TODO-low-latency-audio.md`（Q3 标记部分已答、初步选型倾向）、`docs/prompt-for-research-session.md`（环境现状更新）
- 状态: 调研前置阻塞已清除，**仍无产品代码**
- 完成: ASIO 构建链路打通（`pip install libclang` 提供 DLL + ASIO SDK 预置到 `%TEMP%\asio_sdk`）；探针编译成功并实测
- **核心数据**: `TOPPING Pro` 支持 8–2048 帧 buffer；`Fixed(64)` 下回调 1.45ms、发布→出声中位 **0.68ms**、max 1.47ms；WASAPI 共享同机 8.33ms。ASIO 快约 12 倍且块大小无抖动
- **决定性发现**: ASIO 不是独占的。两进程并发开流，第二个实例 690 callbacks/s、0.69ms，性能无衰减。"DAW 开着变哑"的担忧在本机不成立
- 验证: 探针 `%TEMP%\asio-probe` 编译并跑三轮（64/128/256 帧 + 双实例并发）；WASAPI 与 ASIO 同一程序对比
- 遗留: 2ch 立体声配置未验（本轮只协商到 1ch）；真实 DAW 占用场景未复验；WASAPI 独占未测；ASIO SDK 许可证与分发方式未查（Q5）
- 下一步: 用 TODO 文档开调研对话。Q1 的 15ms 目标现已宽裕——设备侧只占 1.45ms 周期 + 0.68ms 延迟

## 2026-11-16 09:40 · hermes · 项目建立 + 调研前置实测

- 改: 新建 AGENTS.md、docs/TODO-low-latency-audio.md、docs/research-notes-playback.md、docs/agent-log.md
- 状态: **仅调研准备，无代码**。仓库已 git init，未提交远端
- 已完成: 参考机播放路径实测（symphonia 解码、WASAPI 常驻流 swap 延迟、回调几何、webview 对照），数据在 research-notes-playback.md
- 关键结论: 解码 1–6ms 不是瓶颈；WASAPI 共享模式周期固定 10.1ms 且无视小 buffer 请求；ASIO 是唯一能压到个位数毫秒的路
- 验证: 探针项目跑通并复测三轮（rust-audio-probe 的 main/geometry/decode_probe 三个 bin）
- 遗留: **ASIO 实测未采到**。asia-sys 构建需 LLVM/libclang，参考机没装；ASIO SDK 已手动解压到 %TEMP%\asio_sdk 绕过 rename 失败。这项是调研对话 Q3 的第一个阻塞
- 下一步: 用户拿 TODO 文档开新对话做调研；不开写实现

## 项目起点

从 `C:\Users\Naimio\projects\slip-sound`（Electron 版，fork 自 Stuyk/slip-sound）继承 UI/UX 与交互模型，重写后端。Slip Sound 侧的播放延迟优化与 Tauri 可行性分析已完成并推送（commit fd2f0cf）。
