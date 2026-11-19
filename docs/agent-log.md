# Agent log

Shared handoff log. Newest entry on top. One entry per work session: what changed, what was verified, what is left.

## 2026-11-19 19:05 · hermes · 发 0.1.2：只发安装版，release 建好

- 改: `tools/check-shipped.mjs`（版本断言改成 `plugin:app|version`——原来读 `<meta name="version">`，前端没这个标签，`if (version)` 从写下起就一直跳过，版本号从来没被验过；补三条本版事实：默认 `windows-dark`、`--theme-color-1` = `#202020`、`--theme-color-primary` = `#4cc2ff`）、`tools/README.md` 与 `docs/app.md`（发版流程去掉便携版那步）、`AGENTS.md`（`package-portable.sh` 的注释改成「0.1.2 起不再发」，发版条目去掉「便携版 zip」）
- 验证: `tauri build --features asio --bundles nsis` → 5304445 字节；NSIS `/S` 装到 `%LOCALAPPDATA%\Mio Sound Manager`，版本资源 0.1.2，单起装好的 exe（CDP 9227）跑 `check-shipped` 11/11（标题 / ASIO / 设备 / 三个控件 / `get_loop` / 版本 0.1.2 / windows-dark / #202020 / #4cc2ff）；设置里的主题卡实测 7 张、顺序 windows-dark…tokyo-night、当前选中 windows-dark；`cargo test --release --workspace` 133 passed；release 三个附件全 `uploaded`，逐个 `curl -L` 返回 200 且字节数匹配（5304445 / 2229 / 35184），下载回来的安装包 SHA-256 与本地一致（c7a1c7f4…）；tag `v0.1.2` 已推并 `git ls-remote` 回读
- 遗留: ① `check-shipped` 里「主题卡 7 张」这条断言删掉了——主题卡只在设置弹窗打开时进 DOM，脚本不开弹窗，`querySelectorAll` 恒为 0。这条事实由 `check-tags.mjs` 覆盖（它真的开弹窗）；② 便携版脚本 `tools/package-portable.sh` 还在仓库里，只为能重建 0.1.1 及更早的包
- 下一步: 若要按 Windows 系统强调色动态生成配色，需要 `mio-app` 侧读 `UISettings.GetColorValue` 再经 IPC 传来；现在是写死的 Fluent 默认强调色

### 版本号断言原来是死的

`check-shipped.mjs` 读 `document.querySelector('meta[name="version"]')`，而 `ui/index.html` 里没有这个标签，于是 `version` 一直是 `null`，`if (version)` 把整条断言跳过——0.1.0 和 0.1.1 发出去时，版本号这一项从来没被验过。改成 `invoke('plugin:app|version')`：那个字符串是编译期从 `tauri.conf.json` 写进二进制的，构建早于版本号改动就会在这里失败，而这正是安装包文件名会错的那种情形。

## 2026-11-19 18:40 · hermes · 删 Classic 两套配色、默认改 Windows 深浅、加 Tokyo Night

- 改: `ui/src/theme.ts`（`Theme.palette` 去掉 `null` 分支、新增可选 `ramp` 覆盖中性 ramp、`THEMES` 换成 windows-dark/windows-light/studio-dark/studio-light/amber-console/frost-dark/tokyo-night、`DEFAULT_THEME` 变 `windows-dark`、`applyTheme` 去掉「删覆盖块」那条路径）、`ui/src/SettingsDialog.tsx`（主题卡色块不再有 iX 原样的硬编码分支）、`ui/src/locales/{en,zh,ja}.ts`（删 `theme.classicDark`/`classicLight`，加 `theme.windowsDark`/`windowsLight`/`tokyoNight`）、`ui/index.html`（首帧脚本的兜底 id）、`tools/check-themes.mjs`（七套主题、按 `data-ix-color-schema` 判深浅而不是按 id 猜、每套主题各自测 WCAG、覆盖块长度下限、**存着已删除的 `classic-dark` 时必须回退到默认**、hex 与 rgb() 两种 token 形式的亮度解析）、`tools/shot-themes.mjs`、`docs/app.md`、`tools/README.md`、`AGENTS.md`（命令注释里的「六套」）
- 验证: `tsc --noEmit` 干净；`npm run build` 通过；`cargo build -p mio-app --release` 通过；`check-themes` 七套全过（每套 worst 都 ≥ 4.64:1，最低是 amber-console 的 facet；windows-dark 8.12、windows-light 7.5、tokyo-night 5.11）；`check-i18n` 三语 156 键齐平；`check-interactions`、`check-layout`、`check-volume`、`check-tags` 全过；截图 `themes-v2/` 七张，像素实测侧栏/表格底色（windows-dark 45/32、windows-light 231/243、tokyo-night 53/26）与 palette 的 ramp 一致
- 遗留: ① `crates/mio-engine` 的 `dead_code` 警告是原有的，与本次无关；② 已发布的 v0.1.2 里仍是 Classic 那两套，本改动尚未发版；③ `docs/architecture.md` 的 P7 行和 `docs/agent-log.md` 的历史条目里仍写着「六套主题」——前者是当时的事实记录，未改
- 下一步: 若要按 Windows 系统强调色动态生成（而不是固定 `#4cc2ff`/`#0067c0`），需要在 `mio-app` 侧读 `UISettings.GetColorValue` 再经 IPC 传进来；现在两套 Windows 主题是写死的 Fluent 默认强调色

### 为什么删掉 palette: null 而不是留着当第三套

`palette: null` 的唯一用途是「iX 原样」。它占了两套主题（深/浅），而用户明确说这两套丑，那两套删掉之后没有任何主题用它，留着就是一个永远走不到的 `if`。删掉之后 `Theme.palette` 不再可空，类型层面保证每套主题都真的注入了覆盖块——`check-themes.mjs` 里那条「覆盖块必须存在且 >1000 字符」因此变成可断言的事实，而不是碰巧成立。

### ramp 必须能按主题覆盖

固定 RAMP 是从背景线性混到前景，参考间距取自 iX 的深青色块。Fluent 的窗口 `#202020` 与卡片 `#2b2b2b` 只差 5%，用参考间距（16%）会把侧栏推成 `#3f3f3f` 的中灰，和 Windows 11 里那种「几乎贴着底色」的层次完全不是一个东西。windows-* 各带一份更平的 ramp，tokyo-night 沿用原 RAMP（它自己的 `bg_dark`/`bg_highlight` 恰好落在那组比例上）。

## 2026-11-19 14:30 · hermes · 索引卡片去小字、框选对比度、列顺序可拖

- 改: `ui/src/IndexOverlay.tsx` + `ui/src/styles.css`（删掉索引卡片标题下那句说明与 `.index-note` 规则）、`ui/src/WaveformDock.tsx`（两层画布都改成设备像素坐标 + 逐柱 `Math.round` 消除条纹；框选改为「外面压暗 + 里面轻染 + 2px 描边」）、`ui/src/theme.ts`（`--mio-region-scrim` / `--mio-region-fill` / `--mio-region-edge` 三个 token 替换没人用的 `--mio-wave-dim`）、`ui/src/SoundTable.tsx`（列头 `draggable` + 左右半区分插入位置、`mio:column-order` 持久化、收藏星钉首位、单元格改成按列键渲染的 `Cell` 组件）、`ui/src/styles.css`（`.head-cell[draggable]` 的 grab/grabbing 光标、`[data-drop]` 的 2px 落点位）、三语字典（删 `index.note`、加 `col.reorder`）、`tools/check-tags.mjs`（列顺序六项断言 + 索引卡片无小字断言）、`docs/app.md`
- 验证: `tsc --noEmit` 干净；`npm run build` 通过；`cargo test --release --workspace` 133 全通过（app 18 + ipc 25 + engine 19 + index 71）；`check-i18n` 三语 155 键齐平；`check-tags` 全通过（含列顺序拖动 → 表头/单元格/localStorage/重载/拖动后仍排序/收藏星不可拖，以及「拖过表头不改变当前排序」）；`check-layout`、`check-interactions`、`check-volume`、`check-themes`、`check-region` 全通过；像素实测：改前横截面 `(0,234,255)/(3,186,203)` 交替 → 改后 `(0,234,255)` 单色；长曝光截图确认选区内外亮度差在满幅波形上仍存在，浅色两套主题同样成立
- 遗留: ① 列宽仍固定（`COLUMNS` 里的 px），拖动只改顺序，改宽度仍是另一个功能；② 列顺序不随库变化，是全局设置（与主题、缩放同一层），若以后要多窗口要考虑；③ `--mio-wave-dim` 删除后没有其它引用，但第三方主题若引用过它会失效（仓库内无此引用）
- 下一步: 若要列宽可拖，落点逻辑可复用现在的 `dropTarget`，但需要把 `width` 从字符串常量换成信号并把 `template()` 变成可持久化的 px 数组

### 条纹不是配色问题，是坐标问题

第一眼看上去像「波形用了渐变」，实际是抗锯齿：柱宽 `barW - 0.5`、x 落在 `i * 0.993` 这种小数上，每根柱跨两个设备列各覆盖一半，合成出来就是明暗交替。改配色没有任何效果，唯一有效的是别让柱落在半个像素上——整层画布在设备像素里算、左右边界各自 `Math.round`，相邻柱天然拼满。

### 框选不能靠「给里面加色」

波形柱本来就是 accent 同色系，往里叠 accent 等于没叠，满幅波形上选区直接消失（改前截图可见：选区只剩两条细边）。改成压暗外面之后，判据与波形亮度无关：外面永远比里面暗（浅色主题下永远比里面亮），边缘固定 2 个设备像素。

### 列顺序必须让表头和单元格同源

原来的行是六个定序 `<span>`，改列顺序后表头顺序变了、单元格没变，于是「Length」的标签压在采样率上。改成 `Cell` 组件按 `SortKey` 分支渲染，两侧都由 `columns()` 生成。这类错 CSS 抓不到，只能靠断言单元格内容（`check-tags.mjs` 里那项 `cells follow the header`）。

## 2026-11-19 11:50 · hermes · UCS 8.2.1 兜底词表

- 改: 新增 `tools/ucs_mapping.py`（手写 753 条 UCS → 22 个分类的投影 + 排除清单）、`tools/import-ucs.py`（读发布包 → 三道筛 → `tools/ucs_keywords.py`）、`tools/ucs-evidence.py` + `tools/ucs-catid-evidence.py`（实测证据 → 两份 json）、`tools/classify-mirror.py`（分类器的 Python 镜像 + `--sample` 导出期望值）；`tools/gen-category-dict.py` 改为同时生成 `dictionary.rs` 与 `ucs.rs`、并保持 `dictionary.rs` 逐字节不变；`crates/mio-index/src/classify/mod.rs`（`classify` 拆成两遍 + `classify_fallback` + `finish` + `add` + `cat_id_prefix` + `build_cat_ids`/`build_ucs_keywords`/`build_ucs_phrases`，9 个新测试）；新增生成文件 `crates/mio-index/src/classify/ucs.rs`；`crates/mio-cli/src/main.rs`（`classify-check` 子命令）；`.gitignore`（`/docs/UCS Release/`）；`docs/index.md`、`THIRD-PARTY-NOTICES.md`
- 验证: `cargo test --release --workspace` 90 全通过（index 71 + engine 19）；`mio-cli classify-check` 对 271,350 条真实路径 **0 差异**；实测同一语料未分类 12,740（4.70%）→ 11,611（4.28%），1,129 个文件拿到分类，**已分类文件的分类变化 0**；`tools/gen-category-dict.py` 重跑后 `dictionary.rs` 在 git 里无差异
- 遗留: ① AGENTS.md 的「Where things live」表还没加 UCS 那几行（受保护文件，等用户批准）；② UCS 的发布包不进 git，所以 CI/新机器上无法重跑证据链，只能提交已经生成好的 `ucs_keywords.py` 与两份 evidence json；③ 6,085 个 UCS 词里有 4,405 个在本机语料上零命中（`disney`、`minecraft`、`hippopotamus` 之类），留着是因为它们是 UCS 的正规条目、对别的库有用，但在这台机器的库上不会触发；④ CatID 只覆盖到语料里出现过的 139 个，其余 564 个从未在真实路径上验证过映射
- 下一步: 换新 UCS 版本时按 `docs/index.md`「维护流程」重跑四步；若要收紧词表，把 `tools/import-ucs.py` 的 `FILES_CUT` 从 250 往下调，每收紧一档都要重跑 `classify-check`

### UCS 不能当第二套分类，只能当兜底

最初的方案是把 UCS 的 753 个条目直接映射进 22 个分类、让两套词在同一个分数表里竞争。实测否掉了它：把 7,289 个 UCS 同义词按低权重（文件名 1 分）混进去，28,851 个文件的分类发生变化，其中有 22,442 个是已经分好的文件被改到别的分类（`Vocals → Music` 2,465 个、`Magic → Music` 2,291 个、`Drums → Music` 2,027 个，其余散布在 `Impacts`、`UI & Game` 等）。

改成两遍之后同一个词表的收益是净的：1,129 个原本未分类的文件拿到分类，**已分类文件 0 个变化**。这个 0 不是权重调出来的——兜底阶段只有在第一遍得分为 0 时才进入，所以「UCS 不覆盖手工词表」是代码结构给的保证，改权重也破坏不了。

### 词表要按证据筛，不是按看起来对不对筛

UCS 的同义词表是给馆员做检索用的，同一个词在不同条目下意味着不同的东西。直接拿来用会被两类词毒化：

- **包主题词。** `hybrid` 是 UCS 里 VEHICLES-CAR 的同义词，在这台机器的库里是 Sonniss 包名的一部分：12,454 个文件横跨 19 个包。这类词一放进来就会把整个厂商目录染成同一个分类，和 `docs/index.md` 里 `epic` 的教训是同一个。
- **跨域歧义词。** `trance` 是 MAGIC-SPELL 的同义词，在库里是曲风；`acid` 是 GORE 的同义词，在库里是合成器的 acid 音色；`calendar` 是 FOLEY-OFFICE 的同义词，在库里是 Sonniss 包的文件夹名（7,350 个文件、2 个包）。

所以 `tools/ucs-evidence.py` 统计的是「在文件名含有该词、且本项目分类器已经给出答案的文件里，答案与 UCS 映射一致的比例」。一致率 ≥ 0.6 且样本 ≥ 10 才收，样本不足时只有影响面 ≤ 250 个文件才收。7,078 个候选里留下 6,085 个，被否掉的 976 个是「实测与映射矛盾」、17 个是「没有证据却横扫整个包」。

### 镜像会漂移，必须先对等再报数

`tools/classify-mirror.py` 是为了在 Python 里快速试词表而写的。这一轮它漂了两次，两次都是 `mio-cli classify-check` 抓出来的：

1. `lookup()` 在 curated 没命中时回退到 UCS 表，等于让 curated 阶段偷偷用上了 UCS 词（Rust 是严格分两遍的）；
2. `UCS_FILE_WEIGHT = 1`，而 Rust 的兜底文件名词权重是 2。两条路径给的答案在 5 个文件上分叉，而那 5 个正好是「目录词 vs 文件名词」的平局点。

两次都是先看到「271,350 条里有 5 条不一致」才发现，而不是靠读代码。所以规矩是写进 `docs/index.md` 的：**任何由镜像算出来的数字，先过 `classify-check` 才能引用。**

### CatID 要区分大小写，CatID 的映射也要过证据

`GORESqsh_...` 这样的前缀是 UCS 在文件名里声明分类，比任何词都硬（所以权重 8）。但 `RAIN`、`WIND`、`STORM`、`TEST`、`IR`、`MIX` 这些 CatID 本身就是常见英文词，折叠大小写会让每个 `rain_on_roof.wav` 都声明一次；因此 `cat_id_prefix` 用原始大小写比较，且要求前缀后紧跟 UCS 自己的分隔符之一。

映射本身也过了同一套证据筛：`DSGNBass`（UCS 的 DESIGNED/BASS DIVE）在这台机器的库里 18 个文件全是 Bass，`OBJMisc` 22 个文件里 curated 已经分出 10 个，两个都不一致，被排除；`FGHTImpt`、`HNRBrth`、`GOREMisc`、`MUSCInst` 同理。最终 703 个 CatID 里，语料里真正出现过的只有 139 个。


## 2026-11-18 21:40 · hermes · 0.1.2：波形手势重做、声道分道、库文件改名、循环开关可见

- 改: `crates/mio-engine/src/waveform.rs`（`lanes` 逐声道包络 + `bucket_peaks_lane`、`peaks_for` 共用内层循环、4 个新测试）、`crates/mio-engine/src/lib.rs`（导出 `bucket_peaks_lane`）、`crates/mio-index/src/db.rs`（`DB_FILENAME` → `mio-sm.db`、`LEGACY_DB_FILENAME`、`adopt_legacy_file` 连 `-wal`/`-shm` 一起改名、1 个新测试）、`crates/mio-index/src/indexer.rs`（文档串）、`crates/mio-app/src/commands.rs` + `state.rs`（`WaveformDto.lanes`）、`crates/mio-app/tests/ipc.rs`（断言双声道两条道）、`ui/src/WaveformDock.tsx`（顶部三分之一拖出带、边界拖动、竖杠光标、分层绘制、`ix-icon-toggle-button` 循环开关、`loopState()` 说清循环的是什么）、`ui/src/App.tsx`（`dragRegionOut(atSeconds)` 按起点决定拖选区还是拖源文件）、`ui/src/ipc.ts`（`Waveform.lanes`）、`ui/src/ix-elements.d.ts`（`ix-icon-toggle-button` 类型）、`ui/src/styles.css`（`.wave-grab` 带、三种光标、`#transport-loop.on` 实心覆盖）、`ui/src/locales/{en,zh,ja}.ts`（4 个新键 + `dock.loopWhole`）、`tools/check-region.mjs`（新增光标/拖出带/边界拖动/循环外观/声道道数/拖出落成 WAV 共 16 项）、`tools/README.md`、`docs/{app,engine,index}.md`
- 验证: `cargo test --release --workspace` 124 全通过（engine 19 + index 62 + app 18 + ipc 25）；`tsc --noEmit` 干净；`check-i18n` 三语 155 键齐平；**`check-region` 36/36**；`check-layout` / `check-interactions` / `check-volume` / `check-themes` / `check-tags` 全通过；真实库（271900 行）打开后文件已是 `mio-sm.db`，行数与分类都在；画布像素实测：立体声 top 44% / mid 11% / bottom 45%（两条道），单声道 top 3% / mid 94% / bottom 3%（一条道）
- 遗留: ① 拖出带只能验证到「落成 WAV + 交给 start_drag」，真的拖进 DAW 仍未人工验收（与 P0 同一限制：原生 OLE 拖拽无法从 CDP 合成）；② 拖出带「起点在选区外拖源文件」这条分支没有独立断言——`__TAURI_INTERNALS__.invoke` 是不可写的，桩不进去，而一次会话只能按一次带（按下即进原生拖拽循环），这条分支与「无选区」共用同一个 `!inside` 代码路径；③ 多声道（>2）退回单条只有单测覆盖，库里没有 5.1 素材可实测；④ `exports/` 仍无清理策略，这次又多了几个 WAV
- 下一步: 在真实 DAW 里拖一次选区；给 `exports/` 加启动清理或上限；找一份 5.1 文件确认退回单条道

### 循环开关「指示不明确」是个真 bug，不是样式问题

`ix-icon-button` 的 `selected` 属性在 iX 5 里是死的：`icon-button.js` 的 `render()` 里硬写 `selected: false`，传什么进去都到不了内层按钮的 class。所以循环开着和关着在像素上完全一致——`getComputedStyle(button).backgroundColor` 两次都是 `rgba(0,0,0,0)`。

换成 `ix-icon-toggle-button`（它把 `pressed` 转成同一个 `.selected` 类，并补 `aria-pressed`）之后才「有按下态」；但 iX 的 `secondary` 按下态是 accent 的 16%，在深色 dock 上仍看不出，所以 `#transport-loop.on` 用 `--theme-btn-tertiary--*` 一组 token 覆盖成实心 accent + 对比色文字，并把 hover/active/pressed 全状态一起覆盖，避免按下瞬间闪回淡色。

判据不能是属性：`pressed === true` 在修好之前也是 false、修好之后仍是 true，中间那段「属性对但像素没变」的窗口只有读 `getComputedStyle` 才看得见。`check-region.mjs` 现在断言开/关两次的 `backgroundColor` 不同，实测 `rgb(0,189,227)` vs `rgba(255,255,255,0)`。

### 手势为什么要按落点分区，而不是继续用 Alt

Alt+拖拽在实现上没问题，但它把「拖出去」变成一个必须持续按住的组合键：指针一离开窗口、焦点一换，修饰键状态就断了，而拖拽本身是一次连续动作。改成空间分区（顶部三分之一 = 拖出，边界 ±5px = 改边界，其余 = 框选/定位）之后，面板用高亮带和 grab 光标自己说明哪里是把手，修饰键小字提示也可以删掉。

拖出去带什么由起点决定：起点在选区内 → 渲染选区成 WAV；起点在选区外或没有选区 → 直接拖源文件。这样「拖整个文件」不需要记第二个手势。

### 库文件改名要连 journal 一起改

`slip-sound.db` → `mio-sm.db` 如果只改主文件，`-wal` 还叫旧名，SQLite 打开时会认为上次是干净关闭的，最后一个已提交事务就丢了。`adopt_legacy_file` 三个后缀一起 `rename`，且只在新名不存在时动手（两个都在说明迁移中途被打断，猜哪个是当前版本比留一个陈旧文件更糟）。

真实库验证：打开后 `D:\Users\Naimio\Documents\SoundLibrary\mio-sm.db` 287MB、271900 行、133 个 facet，没有重新索引。

## 2026-11-18 15:30 · hermes · 发 0.1.1 到 release，便携版改打 zip

- 改: `Cargo.toml` + `crates/mio-app/tauri.conf.json`（版本 0.1.0 → 0.1.1）、`tools/package-portable.sh`（新增）、`tools/check-shipped.mjs`（新增）、`tools/kill-app.sh`（改为按两个进程名结束）、`tools/README.md`（新增「发版」一节）
- 验证: `tauri build --features asio --bundles nsis` 产出 5253200 字节安装包；`package-portable.sh` 产出 6443256 字节 zip（内含 17262080 字节 exe + LICENSE + 第三方声明，无 DLL）；**解压后的 exe 单独启动过**（CDP 9227）并跑 `check-shipped` 7/7：标题、`asio_compiled=true`、引擎开到设备、`#transport-loop`、`#volume-mute`、5 条手势图例、`get_loop` 应答；release v0.1.1 的四个附件 `gh release view` 全部 `uploaded`，逐个 `curl -L` 返回 200 且字节数匹配（5253200 / 6443256 / 35184 / 2229）；tag `v0.1.1` 已推并 `git ls-remote` 回读
- 遗留: ① 想改 `AGENTS.md` 的命令表（加 `tauri build` 与 `package-portable.sh` 两行）被保护机制拦下（审批超时），**现状是没改**——`AGENTS.md` 的 Commands 节仍只有 `build-app.sh`，发版第二步要读 `tools/README.md`；② 框选拖进真实 DAW 与循环听感仍未人工验收（同上一轮）；③ 便携版配置仍写 `%APPDATA%`
- 下一步: 找用户拿自动播放那台的复现条件；真实 DAW 里拖一次选区 WAV；给 `exports/` 加清理策略

### 便携版为什么不再做单 exe

单 exe 是对「跑起来的到底是什么」的谎报。它一直只是 `target/release/mio-sound-manager.exe` 的改名，而当 `LICENSE` 与 `THIRD-PARTY-NOTICES.md` 必须跟着二进制走（AGPL 分发要求），那个改名就成了陷阱：用户下载的那个文件不再是能用的东西，旁边还该有什么只能靠猜。

zip 里固定三件：`Mio Sound Manager.exe`、`LICENSE`、`THIRD-PARTY-NOTICES.md`。没有 DLL——`tauri` 静态链 WebView2Loader，webview 资源经 `custom-protocol` 编进 exe，目录确实是自包含的。脚本会在 `target/release` 下发现任何 `.dll` 时直接失败，而不是打一个缺依赖的包出去。

### 两个把构建卡住过的坑

**`tauri build` 必须在仓库根跑。** 在 `ui/` 下执行 `npx tauri build` 报 `Couldn't recognize the current folder as a Tauri project`——配置在 `crates/mio-app/`，Tauri 会往上找，但起点得在仓库根。

**`kill-app.sh` 只认一个进程名。** 它原先只 `Stop-Process -Name mio-sound-manager`，而便携版的 exe 叫 `Mio Sound Manager.exe`，进程名不同。于是便携版还在跑、exe 被映射，构建报 `failed to remove file … 拒绝被访问 (os error 5)`——看起来像权限问题，实际是文件锁。现在两个名字都结束。

### 版本号有两处

`Cargo.toml` 的 `[workspace.package] version` 决定 crate 版本与 zip 文件名（脚本从这里读），`crates/mio-app/tauri.conf.json` 的 `version` 决定安装包文件名。只改前者会得到「exe 是 0.1.1、安装包叫 0.1.0」。这次两处都改了。

## 2026-11-18 13:30 · hermes · 修拖拽失效、静音记忆，加波形框选/循环/选区拖出

- 改: `ui/src/SoundTable.tsx`（`draggable` → `draggable={true}`）、`ui/src/WaveformDock.tsx`（框选手势、选区画在播放头层、循环开关、`.dock-hints` 图例、区域读数与清除按钮）、`ui/src/App.tsx`（`readStoredGain`、`toggleMute` + `mio:unmute-gain`、`toggleLoop`、`changeRegion`、`dragRegionOut`、Esc 清选区、L 开关循环、onMount 回读 `get_loop`）、`ui/src/ipc.ts`（`setLoop` / `loop` / `exportRegion`、`LoopState`、`PlaybackPosition` 加两字段）、`ui/src/icons.ts` + `ui/src/assets/icons/loop.svg`、`ui/src/locales/{en,zh,ja}.ts`（14 个新键）、`ui/src/styles.css`（`.wave-wrap.selecting`、`.wave-region`、`.dock-hints`）、`crates/mio-engine/src/output.rs`（`Shared` 三个循环原子、`Mixer::fill` 分段回绕、`set_loop` / `loop_state`、`PlaybackSnapshot` 两字段、`publish` 清范围）、`crates/mio-engine/src/export.rs`（新增，WAV 写入 + 3 个单测）、`crates/mio-app/src/state.rs`（`LoopSetting` / `LoopState`、`set_loop` / `loop_state`、`export_region`、`sanitize_file_name`、`open_audio` 恢复循环）、`crates/mio-app/src/{commands,lib}.rs`（三条新命令）、`tools/check-region.mjs`（新增 16 项）、`tools/check-volume.mjs`（静音断言改为回到设定值）、`tools/check-interactions.mjs`（AUTO PLAY 持久化改读 `mio:settings`）、`docs/{app,engine}.md`、`tools/README.md`
- 验证: `cargo test --release --workspace` 124 个全部通过（engine 16 + app 25 + index 61 + 其余 lib/集成）；`tsc --noEmit` 干净；`check-i18n` 三语 151 键齐平；`check-region` 16/16；`check-volume` 12/12（含静音→取消静音回到 37%、静音后重载仍回到 64%）；`check-interactions` / `check-layout` / `check-themes` / `check-tags` 全通过
- 遗留: ① **自动播放那条没能复现**——全新 localStorage、空库加库、按长度降序点最长一行，播放头 0→0.65s 且 `playing: true`，首次点击就出声；朋友的机器上还需要拿到当时的具体条件（哪个库、点的是哪一行、装的是安装版还是便携版）才能继续查；② 框选拖出只验证到 `export_region` 写出 WAV 并返回路径，**没有真的拖进 DAW**（与 P0 同一个限制：原生 OLE 拖拽无法从 CDP 合成）；③ 循环的听感没验，只验了状态与回绕逻辑（`get_loop` 回读、Esc 后范围变 `null`）；④ 选区导出每次都在缓存目录留一个 WAV，没有清理策略
- 下一步: 找用户拿自动播放那台的复现条件；在真实 DAW 里拖一次选区导出的 WAV；给 `exports/` 加个上限或启动时清理

### 拖拽失效的根因

`SoundTable.tsx` 的行写成 `<div draggable>`。Solid 把它渲染成 `draggable=""`，而这个属性是枚举型（`true`/`false`/`auto`），空串不是合法值——浏览器回落到 `auto`，`row.draggable` 读出来是 `false`，DOM 拖拽根本不会发起，`onDragStart` 一次都没执行过。代码读起来完全正常，`check-*.mjs` 里也没有任何一项碰它（旧 `check-browsing.mjs` 的拖拽项在 iX 重写时被删了，那次重写只保留了「命令能到插件」的探测，而那条探测是直接 `invoke` 的，绕过了行本身）。

判据：修复前 `{prop: false, attr: ''}`，修复后 `{prop: true, attr: 'true'}`。`check-region.mjs` 现在把这两条都断言下来，并在最后真派发一次 `dragstart`。

### 静音记忆里还有一个真 bug

第一版只加了 `mio:unmute-gain`，测试仍然失败：静音后重载显示 100%。原因是 `Number(localStorage.getItem(k)) || 1`——存着的 `0` 是 falsy，被当成「没存」，于是回落到 1；下一次取消静音又把这个 100% 写进记忆值，记忆彻底丢了。`readStoredGain` 只对缺失或非数值回落，这才是正确的语义。

## 2026-11-18 00:20 · hermes · 用 AGPL 版重打包并替换 release 附件

- 改: `crates/mio-app/tauri.conf.json`（`bundle.copyright` / `license` / `licenseFile` 三条，让 NSIS 安装器显示许可页）
- 验证: 重新 `tauri build --features asio --bundles nsis`（安装包 4.98 MiB，比无许可版大 13 KiB）；新便携版单独启动过（CDP 9226）：顶栏零个 pill、设置页无解释小字、`Interface scale` 在、`asio_compiled=true`；release v0.1.0 的四个附件逐个 `curl -L` 下载返回 200 且字节数匹配（setup 5219718 / portable 17162240 / LICENSE 35184 / THIRD-PARTY-NOTICES.md 2229）
- 遗留: ① 便携版仍把配置写 `%APPDATA%`，不是真绿色版；② 安装包只验证了「能生成」，没跑过实际安装流程；③ 没有 README（用户说过先不写）
- 下一步: README；安装流程实际跑一遍；便携版配置改到 exe 同目录

### 这次改了什么

旧附件（22:32 上传的那两个）是在 `license = "MIT"` 的 Cargo.toml 下构建的。二进制内容其实与许可字符串无关，但 release 既然对外声明 AGPL，附件就应当来自对应那次提交。做法是删掉旧附件、用 AGPL 配置重打包、重新上传，并把 `LICENSE` 与 `THIRD-PARTY-NOTICES.md` 一并作为附件——AGPL 分发二进制时要求附许可声明，放在仓库里不如放在下载页面旁边直接。

release 说明也换掉了：原文只说两个产物，现在写明 AGPL、ASIO SDK 走 GPLv3 一支、源码地址，并列出两个 exe 的 SHA-256，便于下载者自行核对。

**下载页取不到附件列表是假象。** 用 `curl` 抓 release 页面 HTML 会得到 0 个匹配，因为 GitHub 的附件区是 JS 渲染的。判据应当是 `gh release view --json assets`（`state: uploaded`）或对 `releases/download/<tag>/<file>` 发一次真实请求看 HTTP 码与字节数。

## 2026-11-18 00:05 · hermes · 许可证改成 AGPL：清掉仓库里的 MIT 残留与「SDK 不能随 GPL 分发」的旧结论

- 改: `Cargo.toml`（`[workspace.package] license` `MIT` → `AGPL-3.0-or-later`，四个 crate 都继承它）、`AGENTS.md`（技术栈那条去掉「SDK 专有许可」）、`docs/architecture.md`（ASIO 小节与否决表两处）、`docs/engine.md`（ASIO feature 一节）、`THIRD-PARTY-NOTICES.md`（新增）
- 验证: `cargo metadata --no-deps` 四个 crate 均报 `AGPL-3.0-or-later`；`git ls-files -z | xargs -0 grep -nw MIT` 只剩三条正当引用（iX 图标的 MIT、agent-log 里的历史记录）；`LICENSE` 实为 AGPL v3（661 行，`GNU AFFERO GENERAL PUBLIC LICENSE`），不是 GPL v3
- 遗留: ① 上一个 release 的产物（v0.1.0）是在 MIT 声明下打的，二进制本身没变，只是元数据不同——重打要重新上传，是否值得由用户定；② `docs/agent-log.md` 里 `MIT` 的历史记录按规则保持原样
- 下一步: README；给 release 附件附 `THIRD-PARTY-NOTICES.md`（AGPL 二进制分发要求）

### 判断依据

**「SDK 不能随 GPL 分发」是错的结论，且与本项目的发布方式冲突。** 打开 `%LOCALAPPDATA%\mio\asio_sdk\LICENSE.txt` 可以看到 SDK 原文给的是二选一：a) Steinberg 专有许可，b) **GNU General Public License v3**。`asio-sys` 的 `build.rs` 把 `common/*.cpp` 与 `host/**` 直接编译进二进制，所以带 ASIO 的产物里确实含 SDK 目标代码——按 GPLv3 一支走就是合法的，按专有许可则要先和 Steinberg 签协议。本项目整体 AGPL-3.0-or-later，与 GPLv3 兼容（AGPL §13），所以走 b 支成立。之前文档把「不能随 GPL 分发」写成既定事实，导致把 AGPL 项目自己的许可也判成不合规，属于把 SDK 的两条路只读了前一条。

**MIT 的残留分两类，处理方式不同。** `Cargo.toml` 的 `license` 字段是真错，必须改；`@siemens/ix-icons` 的 MIT 是那份独立作品的许可，AGPL 项目里照旧保留署名即可，不能改。`docs/agent-log.md` 里「整体继续按 MIT 发布就不合规」是当时的历史判断记录，保持原样。

**AGPL 的对应源码义务。** 仓库公开、release 页面指向仓库，满足 §6 的主干要求。ASIO SDK 源码不 vendored，用户在构建时自取——这是本项目的构建前提，写进了 `THIRD-PARTY-NOTICES.md` 与 `docs/engine.md`。

## 2026-11-17 22:35 · hermes · 顶栏去库标签、打包 v0.1.0 试用版并发 GitHub

- 改: `ui/src/App.tsx`（删品牌名右侧的 `ix-pill.lib-chip`）、`ui/src/styles.css`（删 `.lib-chip`）、`crates/mio-app/tauri.conf.json`（bundle active + nsis + currentUser + 三语安装界面；`beforeBuildCommand` 改成 `npm run build`）、`ui/package.json` + lock（`@tauri-apps/cli` 进 devDependencies）
- 验证: `tsc --noEmit` 干净；`check-layout` / `check-interactions` / `check-tags` 全通过；`tauri build --features asio --bundles nsis` 产出 4.97 MiB 安装包与 17.2 MB 便携版；**打包后的 exe 单独启动过**（CDP 9224）：顶栏只有 `Mio Sound Manager`、零个 pill、`asio_compiled=true`、设备表 7 个 WASAPI + 1 个 ASIO
- 遗留: ① 仓库没有 README（用户明确说先不写）；② 已由用户补上 AGPL v3 LICENSE，`Cargo.toml` 的 `license` 与三处「SDK 不能随 GPL 分发」的表述随后修正，见当天 23:20 那条；③ 安装包只在当前机器上验证了「能生成」，没有实际跑一遍安装流程
- 下一步: 补 README；下一个版本把 `%APPDATA%` 下的配置改成便携版可覆盖（目前便携版也写 APPDATA）

### 这次的两件事

**顶栏标签删了。** 那个 chip 显示的是 session 里记的库名，和侧栏头部重复，位置又紧贴品牌名，看起来像第二行标题。`session.json` 本身没动，侧栏头部照旧显示库名。

**打包。** `tauri build --bundles nsis`。踩到一个坑：`tauri.conf.json` 的 `beforeBuildCommand` 是在 `ui/` 下执行的，写 `npm --prefix ui run build` 会变成 `ui/ui/package.json` 找不到——改成 `npm run build` 才对。另外 `@tauri-apps/cli` 原来不在依赖里，`cargo tauri` 也没装，`npx tauri` 会去下载一个不受控的版本，所以把它钉进 devDependencies。

产物：`Mio-Sound-Manager_0.1.0_x64-setup.exe`（NSIS、当前用户、不弹 UAC）和 `Mio-Sound-Manager_0.1.0_x64-portable.exe`（单 exe）。仓库 `NaimioChan/mio-sound-manager` 是 public，release `v0.1.0` 标成 pre-release（试用版）。

## 2026-11-17 22:10 · hermes · 删设置页说明小字、加界面缩放、产品构建默认带 ASIO

- 改: `ui/src/SettingsDialog.tsx`（删 7 处 `.setting-hint` 解释行、删主题卡片的描述行、`ThemeCard` 的 tooltip 改用名字、加「界面缩放」行）、`ui/src/zoom.ts`（新增：`loadZoom` / `applyZoom` / `ZOOM_STEPS` / CDP 调试钩子）、`ui/src/{App,main}.tsx`（zoom 信号 + `onMount` 里 `applyZoom` + `changeZoom` 回调）、`ui/src/theme.ts`（`Theme.hintKey` 移除）、`ui/src/styles.css`（删 `.theme-card-hint`）、`ui/src/locales/{en,zh,ja}.ts`（删 13 个 hint / currentTheme 键、加 `settings.zoom`、`settings.asioMissing` 改成一句话）、`crates/mio-app/capabilities/default.json`（加 `core:webview:allow-set-webview-zoom`）、`tools/build-app.sh`（ASIO 改为默认，`--no-asio` 反向开关，SDK 路径从 `%TEMP%` 改 `%LOCALAPPDATA%\mio\asio_sdk`）、`tools/check-tags.mjs`（加 8 项：hint 只剩状态、主题卡无描述、缩放行存在、150% 真的缩小 CSS 像素）、`AGENTS.md`、`docs/{app,engine}.md`
- 验证: `cargo test --release` 全部通过；`tsc --noEmit` 干净；`check-i18n.mjs` 三语各 138 键齐平；`check-tags.mjs` 45 项全通过（含新增 8 项）；`check-interactions.mjs` / `check-layout.mjs` / `check-volume.mjs` / `check-themes.mjs` 全通过；`./tools/build-app.sh`（无 flag）构建成功且编译了 `asio-sys`，窗口内 `asio_compiled` 返回 true、设备表里有一个 ASIO 驱动（TOPPING Pro USB Audio Device）；缩放实测 100% → `innerWidth` 1280，150% → 853，125% 重载后仍是 1024 且 `mio:zoom` 保留
- 遗留: 主题卡片删掉描述后只剩名字与色板，六个主题「用在什么场合」的说明不再存在于任何地方（用户要求删，不补）
- 下一步: 无

### 三条要求各自的落地方式

**一、设置里的说明小字全删。** 删掉的是 `rowsPerPageHint` / `showCategoryHint` / `autoPlayHint` / `infiniteScrollHint` / `themeHint` / `currentTheme` / `languageHint` 七句加六个 `theme.*Hint`，三语同步删。留下的 `.setting-hint` 只剩四种，且都是控件说不出来的**状态**而不是对控件的解释：设备打不开的原因、当前实际打开的格式与周期、库的条数与路径、未打开库。`Theme.hintKey` 一并从接口里删掉——留着字段却没有消费者，下一个人会以为它还该渲染。

**二、界面缩放。** 走 `getCurrentWebview().setZoom(0.8–1.5)` 而不是 CSS：WebView2 的 `SetZoomFactor` 连布局、命中区域、波形画布一起重排，CSS `zoom` 或 `rem` 倍率会让画布留在设备像素上。该能力不在 `core:default` 里，`capabilities/default.json` 必须显式加 `core:webview:allow-set-webview-zoom`，否则 ACL 直接拒绝，界面上只表现为「选了没反应」。六个档位 80/90/100/110/125/150，存 `mio:zoom`，`applyZoom` 返回实际生效的值，失败时选择器会弹回而不是显示一个窗口没有的值。

**三、构建默认为产品构建。** `tools/build-app.sh` 不带参数即 WASAPI + ASIO，`--no-asio` 反向开关。SDK 从 `%TEMP%\asio_sdk` 挪到 `%LOCALAPPDATA%\mio\asio_sdk`——Windows 会清临时目录，构建前提不能是被随时回收的东西。

**关于 `reslip-test-library`。** 它不是构建标记、不是 dev-only 字符串，是 `%APPDATA%\com.naimio.miosound\session.json` 里记的上次打开的库文件夹名。v0.1.0 起顶栏不再显示它（品牌名右侧那个 chip 删掉了，库名只在侧栏头部）；exe 与打包后的 JS 里都搜不到该字符串。

## 2026-11-17 19:35 · hermes · BPM 排序、tag 多选、文件夹自绘菜单、去副标题、修两个 UI bug

- 改: `crates/mio-index/src/bpm.rs`（新增，文件名 → BPM）、`crates/mio-index/src/db.rs`（`bpm` 列 + `SortColumn::Bpm` + `categories`/`subcategories` 数组筛选 + 升级回填 + 两个测试）、`crates/mio-index/src/lib.rs`（导出 `bpm`）、`crates/mio-app/src/{commands,library}.rs`（`SearchRequest` 改数组、`SoundDto.bpm`、`bpm` 排序白名单）、`crates/mio-app/tests/ipc.rs`（跟随字段改名）、`ui/src/{ipc,Sidebar,App,SoundTable,SettingsDialog,CategoryDialog}.tsx/.ts`、`ui/src/styles.css`（分段控件拼缝）、`ui/src/locales/{en,zh,ja}.ts`（删 3 个副标题键、加 `col.bpm` / `folderMenu.*`）、`tools/check-tags.mjs`（新增，36 项）、`AGENTS.md`（铁律「No subtitles」+ 命令 + 模块导航）、`docs/{app,index}.md`
- 验证: `cargo test --release` 105 个通过（engine 13 / index 61 / app lib 18 / ipc 25）；`tsc --noEmit` 干净；`check-tags.mjs` 36 项全通过；`check-interactions.mjs` / `check-layout.mjs` / `check-themes.mjs` / `check-volume.mjs` / `check-i18n.mjs`（三语各 150 键）全部通过；真实 271,350 条库上 BPM 回填出 21,814 条非空（占文件名含 `bpm` 的 21,818 条中的 99.98%）
- 遗留: ① 剩下的 4 个 BPM 没解出来的是 `9_bpm_approx120`（「大约」不是断言）、`DS_MTS_150_..._rebelbpm`、`Em600bpm`（标记粘在单词里）——都按无数据处理，符合规则；② 回填是一次性升级行为，磁盘上没有 `meta.bpm_backfilled` 的库会在下次启动时补做
- 下一步: 无

### 六条要求各自的落地方式

**一、切换按钮左侧缺边框。** 两半分段控件用 `border-left: none` 拼缝，选中态把边框刷成强调色，被去掉的那条就是缺口。改成 `margin-left: -1px` 共用一条边，并给 `.on` 加 `position: relative; z-index: 1`，否则邻边会盖住共用边。

**二、设置点 × 后再也打不开。** iX 的 `modal-header` 在 × 上直接调 `parentDialog.dismissModal()`，Escape 走 `dialog` 的 `cancel`，两条路都不经过我们自己的 `close()`。于是 `settingsOpen` 一直是 `true`，下次点齿轮是把同一个值赋给同一个信号，什么都不渲染。`SettingsDialog` 与 `CategoryDialog` 都监听 `on:dialogClose` / `on:dialogDismiss`，状态跟着实际关闭走。

**三、BPM 排序。** 只从文件名取，只认显式 `bpm` 标记，范围 20–300，空值两个方向都排最后。不做 tag：连续值做成筛选行只会多出一排没人点的行。旧库靠 `open` 时的一次性回填补值——增量索引会按 `(mtime, size)` 跳过所有未变文件，靠重新索引补不出来。

**四、tag 多选。** 点击替换、Ctrl+点击增删、再点唯一的选中行则清空。SQL 从 `category = ?` 改成 `IN (?, ?, …)`，占位符数量由列表长度生成、值仍走绑定参数。分类之间 OR，分类与子分类之间 AND；点子分类会同时点亮父分类，否则两个条件相交是空集而界面上看不出原因。每个已选行各有一个可单独关掉的 chip。

**五、去掉所有副标题。** 品牌下的 `samples`、设置弹窗的 `Device, library, appearance`、分类弹窗的 `N sounds` 三处删掉（`N sounds` 移到正文第一行，它说的是操作范围）。铁律写进 `AGENTS.md`：标题下不加大字小字第二行，要解释就写正文。

**六、文件夹右键换自绘菜单。** 复用行菜单与库菜单的 `ContextMenu`，条目是在文件管理器中显示 / 只看此文件夹 / 复制文件夹路径。同时删掉行菜单里的「拖进 DAW」——拖拽是鼠标动作，禁用态条目只能写一句点不动的说明。文件夹行与分类行补了 `data-folder` / `data-category`，检查脚本不再依赖翻译后的文案。

## 2026-11-17 17:40 · hermes · 修图标、修设置页 Device 对齐、加中英日三语

- 改: `ui/src/assets/icons/*.svg`（新增 11 个，从 `@siemens/ix-icons/svg/` 复制）、`ui/src/icons.ts`（改为导入本地 SVG → data URL）、`ui/src/i18n.ts` + `ui/src/locales/{en,zh,ja}.ts`（新增，151 键）、`ui/src/styles.css`（`.setting ix-select/ix-input` 拉满）、`ui/src/{App,SoundTable,Sidebar,WaveformDock,CategoryDialog,SettingsDialog,IndexOverlay,theme,main,ipc}.tsx?`（全部文案接 `t()`）、`crates/mio-app/src/{history,state,commands}.rs`（undo/redo 由英文 label 改为 `EditKind` 枚举；删 `SoundDto.channel_label`）、`tools/check-i18n.mjs`（新增）、四个 `check-*.mjs`（固定英文 locale + 改用 `data-sort-key`/`data-menu-id` 选元素）、`docs/app.md`（新增「多语言」节、图标与 ix-select 两个坑）
- 验证: `cargo test --release` 105 个通过（engine 13 / index 49 / app lib 18 / ipc 25）；`tsc --noEmit` 干净；`check-i18n.mjs` 三语各 151 键、占位符一致；`check-interactions.mjs` / `check-layout.mjs` / `check-themes.mjs` / `check-volume.mjs` 全通过；三语逐一切换后实测 DOM 文案正确（表头/侧栏/搜索框/dock/状态栏）、零文本溢出、dock 不超界；图标逐个截图取点阵确认渲染出真实字形（齿轮/文件夹/喇叭/暂停/停止），主界面 6 个 + 设置 13 个零空图标
- 遗留: ① 分类名是数据库里的英文数据，只翻译了 `Uncategorized` 这一个 schema 哨兵值，其余分类名不随语言变（有意为之，翻它会让 facet 筛选发出库里没有的字符串）；② `tools/shot*.mjs` 未固定 locale，截图会跟随系统语言
- 下一步: 无

> 补记（同日）：`AGENTS.md` 已在用户批准后补齐——命令表加 `check-i18n.mjs`、Where things live 加 `i18n.ts`/`locales/`/`assets/icons/`、Stack 加多语言与图标两条、Rules 加「不写字面量文案」、Status 加 P8。

### 三个问题各自的根因

**一、图标全是叉号方块。** 不是资源路径配错，是 `ix-icon` 拿到**名字**时不去查注册表，而是拼 `svg/<name>.svg` 去 fetch——打包产物里没有这个目录，每个图标 fetch 失败后回退成 iX 的错误符号。iX 自己的组件（chevron、check）没事，因为它们从 `@siemens/ix-icons/icons` 导入 data URL。

修的过程中还踩了一层：第一版用 `encodeURIComponent` 包 SVG，结果图标变成**空白**而不是叉号——iX 内部只调 `decodeURI`，它不还原 `%3D`，于是 `<svg width%3d"512px">` 这种废属性被解析出来，每条 path 丢掉 `d`。正确写法是 `data:image/svg+xml;utf8,<原始 SVG>`，并去掉 `<?xml …?>` 声明（iX 按 `text/html` 解析，XML 声明在那儿是 bogus comment）。

用户要求离线可用，所以 SVG 复制进仓库 `ui/src/assets/icons/`，`?raw` 导入，运行时零网络请求，也不依赖包的内部资源布局。

**二、设置页 Device 下拉没右对齐。** `ix-select` 的 host 是 `inline-block`，宽度收缩到最宽选项（实测 236px，外层容器 320px，右边空 84px）。同一行的按钮和 toggle 都是满宽，只有它短。修法一条规则：`.setting ix-select { display: block; width: 100% }`。修完实测 `gapToRowEdge: 0`。

**三、i18n。** 翻的是外壳文案，不是数据。分类名（`Weapons`、`Foley`）由分类器写进数据库，界面翻译它会让 facet 筛选发一个库里没有的字符串；唯一例外是 `Uncategorized`（schema 默认值而非分类结果），显示时翻译、比较时用原串。

后端原本跨 IPC 传 `"Add to Favorites"` 这种英文 label 给界面直接显示，那条路径怎么翻都翻不到。改成传 `EditKind` 枚举，词在前端。`SoundDto.channel_label` 同理删掉，只留 `channels`。

键集由类型保证（英文 `as const` 是基准，另两语 `Record<MessageKey, string>`），`tools/check-i18n.mjs` 补类型管不到的一条：某语种丢了英文里的 `{placeholder}` 或凭空多一个。

### 检查脚本跟着改了一处

四个 `check-*.mjs` 断言的是英文文案（`Length`、`AUTO PLAY`、`Resume`），在中文机器上会因找不到 locator 而失败——那是脚本的前提问题，不是功能坏了。现在脚本开头把 `mio:locale` 固定成 `en` 再 reload；列头与菜单项另有 `data-sort-key` / `data-menu-id`，选元素不依赖翻译。文案覆盖交给 `check-i18n.mjs`。


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
