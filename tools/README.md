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
node tools/check-layout.mjs         # DOM 几何：区域拼合、列对齐、dock 控件顺序、无溢出
node tools/check-i18n.mjs           # 三语字典：键集齐平、占位符不丢不多
node tools/check-tags.mjs           # 分段控件边框、设置弹窗重开、BPM 列、tag 多选、无副标题
node tools/check-shipped.mjs [port] # 打包产物：版本、ASIO、设备、新控件（跑解压出来的 exe）
node tools/shot.mjs [out.png]       # 截图
node tools/shot-themes.mjs [dir]    # 每套主题一张截图
```

七份 `check-*.mjs` 都以退出码表示结果，可以串起来跑。它们互不干扰（每个都先 `reload()`），
顺序任意。需要驱动窗口的那六份都断言英文文案，所以开头会把 `mio:locale` 固定成 `en`——在中文
机器上不固定就会因为找不到 locator 而失败，那是脚本的前提问题而不是功能坏了。列头与菜单项另有
`data-sort-key` / `data-menu-id`，选元素不依赖翻译。`check-i18n.mjs` 读源码，`check-shipped.mjs`
只断言 DOM 结构与 IPC 应答、不读任何文案，两者都不需要固定 locale。

**`check-region.mjs` 必须单独跑，且跑完要重启应用。** 它最后一项会真的派发一次
`dragstart`，那会进入原生 OLE 拖拽循环，`start_drag` 直到拖拽结束才返回，之后所有 IPC
都可能超时（`page.reload()` 会卡到 30s 超时）。所以拖拽项排在整个脚本最后，后面接着跑的
脚本必须先 `./tools/kill-app.sh` 再起窗口。

`check-i18n.mjs` 不需要窗口：它直接读 `ui/src/locales/*.ts` 的源码，检查三语键集是否齐平、
每个语种有没有丢掉或凭空多出 `{placeholder}`。`tsc` 管得到漏键，管不到占位符。

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

两步，顺序固定：先构建，再打包。

```bash
./tools/build-app.sh                       # UI bundle + 带 ASIO 的 exe
./ui/node_modules/.bin/tauri build --features asio --bundles nsis   # 安装包
./tools/package-portable.sh                # 便携版 zip → dist/
```

`./tools/build-app.sh` 只产出 `target/release/mio-sound-manager.exe`，安装包要靠 `tauri
build`。**跑它之前必须关掉正在运行的应用**：Windows 不允许替换已映射的 exe，报的是
`failed to remove file … 拒绝被访问 (os error 5)`，看起来像权限问题其实是文件锁。先
`./tools/kill-app.sh`。

`tauri build` 要在仓库根执行。`ui/` 下没有 `tauri.conf.json`，在 `ui/` 里跑 `npx tauri build`
会报 `Couldn't recognize the current folder as a Tauri project`——配置在
`crates/mio-app/`，Tauri 会自己往上找。

**便携版是 zip，不是单 exe。** 单 exe 是对「跑起来的到底是什么」的谎报：它只是 cargo 产物的
改名，一旦有什么必须放在旁边（许可文本、第三方声明、以后的插件目录），那个改名就成了陷阱
——用户下载的文件不再是能用的东西。zip 把里面有什么说清楚。内容固定三件：
`Mio Sound Manager.exe`、`LICENSE`、`THIRD-PARTY-NOTICES.md`，没有 DLL
（`tauri` 静态链 WebView2Loader，webview 资源经 `custom-protocol` 编进 exe）。脚本会在
`target/release` 下发现任何 `.dll` 时直接失败，而不是打一个缺依赖的包出去。

版本号只有一个来源：`Cargo.toml` 的 `[workspace.package] version`。`crates/mio-app/tauri.conf.json`
的 `version` 是第二处，必须手动同步——Tauri 用它命名安装包，忘了改就会出现「exe 是 0.1.1、
安装包叫 0.1.0」这种情况。zip 的名字由脚本从 `Cargo.toml` 读，不会漂。

发版后对着打包产物验一遍，不是对着 `target/release`：

```bash
# 解压 zip，用调试端口起那个 exe，再跑
node tools/check-shipped.mjs 9227
```

`check-shipped.mjs` 断言的是「只有正确代码发出去才会成立的事实」：窗口标题、`asio_compiled`
为真、引擎开到了设备、`#transport-loop` / `#volume-mute` / `.dock-hints` 存在、`get_loop` 有
应答。对着开发目录跑它会漏掉打包环节的问题（缺文件、版本号没同步）。
