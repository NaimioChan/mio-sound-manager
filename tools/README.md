# tools/

不属于构建的辅助脚本。

## taxonomy.py 与 gen-category-dict.py

`tools/taxonomy.py` 是分类词表的源文件，`tools/gen-category-dict.py` 把它生成为
`crates/reslip-index/src/classify/dictionary.rs`。生成物提交进仓库，只在下述情况重跑。

```bash
python tools/gen-category-dict.py > /dev/null   # 只写 stderr 的统计行
cargo test -p reslip-index
```

词表是数据不是逻辑：23 个分类、133 个 category/subcategory 条目、1,291 个关键词、
70 条短语。手抄必错，所以由脚本生成。只匹配 ASCII，中文路径不参与分类。

**改词表之后必须跑一遍真实库**，不能只看单元测试：

```bash
cargo build --release -p reslip-cli
./target/release/reslip-cli.exe index <library> --full
./target/release/reslip-cli.exe index-stats <library>
```

词表是对着真实库调的。每个词的命中量与横跨的包数都决定它能不能留在表里：一个出现
在几十个包标题里的词（`epic`、`game`、`premium`）会把整个库吸进同一个分类，所以它
进停用词而不是关键词。打分模型、中文提示词的取用位置与理由写在
`crates/reslip-index/src/classify/mod.rs` 的模块文档里，实测数字写在 `docs/index.md`。

## CDP 脚本的运行顺序

`check-browsing.mjs` 的最后一项会调 `plugin:drag|start_drag`，它进入原生 OLE 拖拽循环、命令直到拖拽结束才 resolve。**这一项跑完后 webview 不再响应任何 IPC**，所以：

- 它自己排在最后（脚本内已注明），不要在它后面加需要后端的断言；
- 跑完它之后要重启应用（`./tools/kill-app.sh && ./tools/run-app.sh`）再做别的验证，否则后续脚本会看到「所有 IPC 超时」而不是真实失败；
- 其余脚本（`check-workspace` / `check-index-overlay` / `check-interactions` / `check-layout`）互不干扰，可以任意顺序连跑。
