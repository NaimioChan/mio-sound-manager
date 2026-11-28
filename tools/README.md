# tools/

不属于构建的辅助脚本。

## taxonomy.py 与 gen-category-dict.py

`tools/taxonomy.py` 是分类词表的源文件，`tools/gen-category-dict.py` 把它生成为
`crates/mio-index/src/classify/dictionary.rs`。生成物提交进仓库，只在下述情况重跑。

```bash
python tools/gen-category-dict.py > /dev/null   # 只写 stderr 的统计行
cargo test -p mio-index
```

词表是数据不是逻辑：23 个分类、133 个 category/subcategory 条目、1,291 个关键词、
70 条短语。手抄必错，所以由脚本生成。只匹配 ASCII，中文路径不参与分类。

**改词表之后必须跑一遍真实库**，不能只看单元测试：

```bash
cargo build --release -p mio-cli
./target/release/mio-cli.exe index <library> --full
./target/release/mio-cli.exe index-stats <library>
```

词表是对着真实库调的。每个词的命中量与横跨的包数都决定它能不能留在表里：一个出现
在几十个包标题里的词（`epic`、`game`、`premium`）会把整个库吸进同一个分类，所以它
进停用词而不是关键词。打分模型与取用位置写在
`crates/mio-index/src/classify/mod.rs` 的模块文档里，实测数字写在 `docs/index.md`。

## UCS 兜底词表

第二套词表来自 [Universal Category System](https://www.universalcategorysystem.com/)
8.2.1，链路是五步，顺序不能乱：

```bash
# 1. 把 UCS 发布包解压到 docs/UCS Release/（git 忽略，不进仓库、不随产物分发）
# 2. 统计每个候选词在真实库上的表现（慢，几分钟）
python tools/ucs-evidence.py            # → tools/ucs-evidence.json
python tools/ucs-catid-evidence.py      # → tools/ucs-catid-evidence.json
# 3. 按证据筛出词表
python tools/import-ucs.py              # → tools/ucs_keywords.py
# 4. 生成 Rust
python tools/gen-category-dict.py       # → dictionary.rs 与 ucs.rs
# 5. 验收
cargo test -p mio-index
python tools/ucs-report.py              # 未分类变化 + 回归数，退出码 0 表示零回归
```

**第 3 步不是可选的。** 直接把 UCS 的同义词表搬进来会把库弄坏：`one shot` 一个短语
就把 15,816 个文件拖进 Music，`hybrid`（在库里是 Sonniss 的包名，12,454 个文件横跨
19 个包）会把整个厂商目录染成 VEHICLES-CAR。所以每个词要过“在文件名含该词、且本项目
已经分出答案的文件里，答案与 UCS 映射一致率 ≥ 0.6 且样本 ≥ 10”这一关；样本不足时
只有影响面 ≤ 250 个文件才收。三个阈值就是 `tools/import-ucs.py` 顶部的
`PRECISION_CUT` / `HITS_CUT` / `FILES_CUT`，收紧任何一档都要重跑第 5 步。

映射本身写在 `tools/ucs_mapping.py`：`TOP` 给每个 UCS 大类一个默认归属（`None` 表示
整支丢弃），`SUB` 覆盖个别条目，`DROP_SYNONYMS` / `DROP_PHRASES` 记排除项与理由。
改映射等于改词表，一样要重跑证据链——否则新映射没有证据支撑。

**`tools/classify-mirror.py` 给的是测量数字，不是产品行为。** 它是 `classify/mod.rs`
的 Python 镜像，用来在秒级试不同阈值（改一次 Rust 重编要几十秒）。它会漂移，所以任何
由它算出来的数字都要先过对等检查：

```bash
python tools/classify-mirror.py --sample 271350 --ucs > expect.json
./target/release/mio-cli.exe classify-check expect.json   # 必须 0 mismatches
```

本轮开发里它漂了两次（`UCS_FILE_WEIGHT` 写 1 而 Rust 是 2；`lookup()` 让 UCS 词混进
curated 阶段），两次都是这条检查抓出来的，不是读代码发现的。

**UCS 是兜底，不是第二套分类。** 两遍结构写在 `classify/mod.rs`：curated 得分不为 0
时 UCS 表根本不会被查询，所以「UCS 不覆盖手工词表」是代码结构而不是权重巧合。改这条
结构的话，回归数会从 0 变成几万。

## 外壳的 CDP 检查

`./tools/run-app.sh` 起窗口（webview 调试端口 9223），然后：

```bash
node tools/check-interactions.mjs   # 列表/选择/排序/搜索/菜单/modal/AUTO PLAY/播放头/seek/设置
node tools/check-themes.mjs         # 七套主题应用、重载保留、WCAG 对比度（每套都测）
node tools/check-volume.mjs         # 滑杆 → IPC → 引擎 → 回读，静音记忆，重载
node tools/check-region.mjs         # 行可拖、框选、顶部拖出带、拖边界、循环外观、声道分道、导出 WAV
node tools/check-layout.mjs         # DOM 几何：区域拼合、列对齐（含横向滚动后）、dock 控件顺序、无溢出
node tools/check-i18n.mjs           # 三语字典：键集齐平、占位符不丢不多
node tools/check-tags.mjs           # 侧栏模式下拉/旧键迁移、列宽拖动与显隐、表头菜单、横向滚动、库头无三角、BPM 列、tag 多选、无副标题
node tools/check-favorite.mjs       # 星标不重查（epoch 不变、无骨架、无 searching）、字形延迟、侧栏计数、undo、收藏过滤下必须重查
node tools/check-inspector.mjs      # 详情面板布局/宽度/点击语义、点击后侧栏同步、新 profile 默认列；封面列、批量缩略图、隐藏列零请求（加 http://localhost:1420 时也测 inspect_sound）
node tools/check-inspector-live.mjs # 真实文件：内嵌/sidecar 封面、48px 缩略图、512px 面板图、四源标签、标签与封面点击
node tools/check-shipped.mjs [port] # 打包产物：版本、ASIO、设备、新控件（跑解压出来的 exe）
node tools/check-stress.mjs [port]  # 压力测试：仅在前后端数据结构/算法有破坏性改动时运行
node tools/shot.mjs [out.png]       # 截图
node tools/shot-themes.mjs [dir]    # 每套主题一张截图
```

**`check-inspector*` 需要一个有封面也有标签的小库**，自己造，不要拿真实库做开发性的 details pass：

```bash
python tools/make-inspector-fixture.py C:\Users\Naimio\AppData\Local\Temp\mio-inspector-lib
```

19 个文件：一个带 bext + iXML + LIST + ID3（内嵌 PNG 封面）的 WAV、一组共用 sidecar `cover.png`
的 WAV、若干无封面 WAV，其中两个文件名带 BPM、每个文件 5 秒。会话指向它之后窗口会自己跑一次
details pass（19/19 约 54ms）。`check-scroll-deep.mjs` 需要真实 28 万行库，这个 fixture 会让它以
退出码 2 结束。

**`check-inspector.mjs` 的两种模式。** 不带参数时它连 9223 的真实窗口，只做布局与点击语义（通过
query chip、filter chip 判断一次点击实际改了什么）；传 `http://localhost:1420` 时它用注入的 IPC
fixture 自己开一个 Vite 页面，因此能直接读 `search` 的 payload、`inspect_sound` 的标签和
`get_cover_thumbnails` 的 key 集合。要测封面列与批量请求就传 URL。

常规 `check-*.mjs` 都以退出码表示结果，可以串起来跑。它们互不干扰（每个都先 `reload()`），
顺序任意。需要驱动窗口的检查会断言英文文案，所以开头会把 `mio:locale` 固定成 `en`——在中文
机器上不固定就会因为找不到 locator 而失败，那是脚本的前提问题而不是功能坏了。列头与菜单项另有
`data-sort-key` / `data-menu-id`，选元素不依赖翻译。`check-i18n.mjs` 读源码，`check-shipped.mjs`
只断言 DOM 结构与 IPC 应答、不读任何文案，两者都不需要固定 locale。

`check-stress.mjs` 不属于常规检查串。仅当修改前端或后端的数据结构/算法，且改动可能破坏搜索、窗口分页或试听并发行为时运行；它会快速发起查询与试听请求，但不会改动分类或收藏数据。

`check-favorite.mjs` 创建临时库，验证单行和按路径多选收藏保留列表缓存、「只看收藏」中取消星标移出该行，以及全选收藏和撤销会刷新缓存窗口。脚本只编辑临时库，结束后关闭它并切回原来的库。

**`check-region.mjs` 必须单独跑，且跑完要重启应用。** 它最后一项会真的派发一次
`dragstart`，那会进入原生 OLE 拖拽循环，`start_drag` 直到拖拽结束才返回，之后所有 IPC
都可能超时（`page.reload()` 会卡到 30s 超时）。所以拖拽项排在整个脚本最后，后面接着跑的
脚本必须先 `./tools/kill-app.sh` 再起窗口。

`check-i18n.mjs` 不需要窗口：它直接读 `ui/src/locales/*.ts` 的源码，检查三语键集是否齐平、
每个语种有没有丢掉或凭空多出 `{placeholder}`。`tsc` 管得到漏键，管不到占位符。

`check-responsive-layout.mjs` 用隔离的 IPC 测试数据检查选中文件前后的列宽和 metadata 进度对齐。
先运行 `npm --prefix ui run dev`，再运行 `node tools/check-responsive-layout.mjs http://127.0.0.1:1420`；无需打开桌面应用或真实库。

**iX 组件的四个坑**，写脚本时都会踩到：

1. 控件在 shadow root 里，`host.click()` 无效；要点 `host.shadowRoot.querySelector('button')`。
2. 禁用态有 `pointer-events: none`，Playwright 会一直重试到超时；先断言状态再点内层。
3. 上一轮留下的 modal 会挡住后面所有点击；脚本开头先 `page.reload()`。
4. **`ix-icon-button` 的 `selected` 属性是死的**——iX 5 在 `render()` 里硬写 `selected: false`，传进去没有任何效果。要按下态就用 `ix-icon-toggle-button` 的 `pressed`。验证时不要断言属性，要读内层按钮的 `className` 里有没有 `selected` 以及 `getComputedStyle().backgroundColor`，否则「属性是 true 但像素没变」会被当成通过。

另外 `ix-modal` 的 host 永远是 0×0（内层 `dialog` 是 `position: fixed`），量尺寸要量
shadow root 里的 `dialog`；`aria-label` 会被 iX 从 host 搬到内层按钮，读标签也要读内层。

## 拖拽的那一项

`plugin:drag|start_drag` 进入原生 OLE 拖拽循环，命令直到拖拽结束才 resolve。跑完它
webview 不再响应任何 IPC，所以不要在它后面加需要后端的断言，也不要把它和别的检查放
在同一次运行里——要重启应用再做别的验证。

## 发版

两步，顺序固定：先构建，再打包。只发安装版，便携版从 0.1.2 起不再产出。

```bash
./tools/build-app.sh                       # UI bundle + 带 ASIO 的 exe
./ui/node_modules/.bin/tauri build --features asio --bundles nsis   # 安装包
```

`./tools/build-app.sh` 只产出 `target/release/mio-sound-manager.exe`，安装包要靠 `tauri
build`。**跑它之前必须关掉正在运行的应用**：Windows 不允许替换已映射的 exe，报的是
`failed to remove file … 拒绝被访问 (os error 5)`，看起来像权限问题其实是文件锁。先
`./tools/kill-app.sh`。

`tauri build` 要在仓库根执行。`ui/` 下没有 `tauri.conf.json`，在 `ui/` 里跑 `npx tauri build`
会报 `Couldn't recognize the current folder as a Tauri project`——配置在
`crates/mio-app/`，Tauri 会自己往上找。

版本号只有一个来源：`Cargo.toml` 的 `[workspace.package] version`。`crates/mio-app/tauri.conf.json`
的 `version` 是第二处，必须手动同步——Tauri 用它命名安装包，忘了改就会出现「exe 是 0.2.0、
安装包叫 0.1.1」这种情况。`tools/check-shipped.mjs` 里还写着第三处（版本断言的字面量），
升版本时一起改，否则验产物会在这里失败——那正是它存在的意义。

**安装包上传到 GitHub 时文件名会变。** `tauri build` 产出的名字带空格
（`Mio Sound Manager_0.2.0_x64-setup.exe`），`gh release create` 会把空格换成点，得到
`Mio.Sound.Manager_…`，与 `docs/app.md` 里记的 `Mio-Sound-Manager_<ver>_x64-setup.exe` 不一致。
上传前先把文件复制成连字符版（`Mio-Sound-Manager_0.2.0_x64-setup.exe`）再传，或用
`gh release upload` 单独传那个副本。

`tools/package-portable.sh` 还在仓库里，是给 0.1.1 及更早版本的构建用的，正常发版不跑它。

发版后对着打包产物验一遍，不是对着 `target/release`：

```bash
# 装一遍安装包，用调试端口起装好的 exe，再跑
node tools/check-shipped.mjs 9227
```

`check-shipped.mjs` 断言的是「只有正确代码发出去才会成立的事实」：窗口标题、`asio_compiled`
为真、引擎开到了设备、`#transport-loop` / `#volume-mute` / `.dock-hints` 存在、顶栏
`.brand-mark` 存在且指向打包进去的 `brand-*.svg`、`get_loop` 有应答、版本号。
对着开发目录跑它会漏掉打包环节的问题（缺文件、版本号没同步）。

**`cargo test --release` 会把 `target/release/mio-sound-manager.exe` 覆盖成不带 ASIO 的版本。**
测试跑完要重跑 `cargo build -p mio-app --release --features asio` 再验产物，否则
`check-shipped` 的 `asio_compiled` 会失败，看起来像构建出了问题。
