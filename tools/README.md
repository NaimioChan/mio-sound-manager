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
