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

## 外壳的 CDP 检查

`./tools/run-app.sh` 起窗口（webview 调试端口 9223），然后：

```bash
node tools/check-interactions.mjs   # 列表/选择/排序/搜索/菜单/modal/AUTO PLAY/播放头/seek/设置
node tools/check-themes.mjs         # 六套主题应用、重载保留、WCAG 对比度
node tools/check-volume.mjs         # 滑杆 → IPC → 引擎 → 回读，静音，重载
node tools/check-layout.mjs         # DOM 几何：区域拼合、列对齐、dock 控件顺序、无溢出
node tools/check-i18n.mjs           # 三语字典：键集齐平、占位符不丢不多
node tools/shot.mjs [out.png]       # 截图
node tools/shot-themes.mjs [dir]    # 每套主题一张截图
```

四个 `check-*.mjs` 都以退出码表示结果，可以串起来跑。它们互不干扰（每个都先 `reload()`），
顺序任意。这四个都断言英文文案，所以开头会把 `mio:locale` 固定成 `en`——在中文机器上
不固定就会因为找不到 locator 而失败，那是脚本的前提问题而不是功能坏了。列头与菜单项另有
`data-sort-key` / `data-menu-id`，选元素不依赖翻译。

`check-i18n.mjs` 不需要窗口：它直接读 `ui/src/locales/*.ts` 的源码，检查三语键集是否齐平、
每个语种有没有丢掉或凭空多出 `{placeholder}`。`tsc` 管得到漏键，管不到占位符。

**iX 组件的三个坑**，写脚本时都会踩到：

1. 控件在 shadow root 里，`host.click()` 无效；要点 `host.shadowRoot.querySelector('button')`。
2. 禁用态有 `pointer-events: none`，Playwright 会一直重试到超时；先断言状态再点内层。
3. 上一轮留下的 modal 会挡住后面所有点击；脚本开头先 `page.reload()`。

另外 `ix-modal` 的 host 永远是 0×0（内层 `dialog` 是 `position: fixed`），量尺寸要量
shadow root 里的 `dialog`；`aria-label` 会被 iX 从 host 搬到内层按钮，读标签也要读内层。

## 拖拽的那一项

`plugin:drag|start_drag` 进入原生 OLE 拖拽循环，命令直到拖拽结束才 resolve。跑完它
webview 不再响应任何 IPC，所以不要在它后面加需要后端的断言，也不要把它和别的检查放
在同一次运行里——要重启应用再做别的验证。
