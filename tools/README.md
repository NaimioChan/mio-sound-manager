# tools/

不属于构建的辅助脚本。

## gen-category-dict.mjs

从 Slip Sound 参考实现生成 `crates/reslip-index/src/classify/dictionary.rs`。

分类词典是数据不是逻辑：29 个分类条目、463 个关键词、51 个短语。手抄一定会漂移，所以用脚本从 TypeScript 源直接生成，生成物提交进仓库。

```bash
node tools/gen-category-dict.mjs [path-to-slip-sound]
```

默认读同级目录的 `../slip-sound`。参考实现的词典变了才需要重跑；改 `dictionary.rs` 会被下一次生成覆盖。

生成后跑 `cargo test -p reslip-index` 与 `reslip-cli classify-diff <folder> <reference.db>` 确认分类结果没变。

## CDP 脚本的运行顺序

`check-browsing.mjs` 的最后一项会调 `plugin:drag|start_drag`，它进入原生 OLE 拖拽循环、命令直到拖拽结束才 resolve。**这一项跑完后 webview 不再响应任何 IPC**，所以：

- 它自己排在最后（脚本内已注明），不要在它后面加需要后端的断言；
- 跑完它之后要重启应用（`./tools/kill-app.sh && ./tools/run-app.sh`）再做别的验证，否则后续脚本会看到「所有 IPC 超时」而不是真实失败；
- 其余脚本（`check-workspace` / `check-index-overlay` / `check-interactions` / `check-layout`）互不干扰，可以任意顺序连跑。
