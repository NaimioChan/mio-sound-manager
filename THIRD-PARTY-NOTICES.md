# Third-party notices

Mio Sound Manager 以 GNU AGPL v3 发布（见 `LICENSE`）。下面是随二进制一起分发、
或作为构建依赖使用的第三方组件及其许可。发行安装包与便携版时可执行文件里包含
第 1 节的代码，其余为编译期依赖。

## 1. Steinberg ASIO SDK 2.3

`crates/mio-engine` 的 `asio` feature 会把 ASIO SDK 编译进二进制：
`common/asio.cpp`、`common/asiodrvr.cpp`、`host/asiodrvr*.cpp`、`host/pc/asiolist.cpp`
以及 `asio-sys` 自带的 `helpers.cpp`。

SDK 的 `LICENSE.txt` 给出二选一：

- a) Steinberg 专有 ASIO 许可（要求事先取得 Steinberg 签署的协议，且不得分发 SDK）
- b) GNU General Public License v3

本项目按 **b) GPLv3** 一支使用。项目整体为 AGPL-3.0-or-later，AGPL v3 第 13 条
与 GPL v3 兼容，因此分发二进制时 SDK 部分依 GPLv3 授权。对应的源码就是
<https://github.com/NaimioChan/mio-sound-manager>，加上用户自 Steinberg 取得的
ASIO SDK（本仓库不包含 SDK 源码，原因见下）。

    ASIO is a trademark and software of Steinberg Media Technologies GmbH.

构建时 SDK 需自行下载并解压（`tools/build-app.sh` 会检查 `%LOCALAPPDATA%\mio\asio_sdk`），
版本为 ASIO SDK 2.3。仓库不 vendored SDK 源码，因此**从源码重建带 ASIO 的产物需要
用户自行获取 SDK**——这正是 AGPL 第 6 条「对应源码」义务在本项目里的边界，发布页面
同时提供源码下载链接以履行该义务。

## 2. 前端图标

`ui/src/assets/icons/` 下的 SVG 复制自 `@siemens/ix-icons`（MIT，Siemens AG），
按 MIT 保留原许可。这些图标是独立作品，不与本项目代码构成衍生关系；MIT 的署名要求
由本节满足。

## 3. Rust 依赖

`Cargo.lock` 里记录的全部 crate 均按其各自许可（多数为 MIT 或 Apache-2.0）分发。
`cargo install cargo-license && cargo license` 可生成完整清单。

## 4. 分类词表

`tools/taxonomy.py` 与本项目生成的 `crates/mio-index/src/classify/dictionary.rs` 是
本项目自行编写的分类体系，未取自任何 copyleft 项目的词表（背景见 `docs/agent-log.md`
关于 Slip Sound 汇编版权的记录）。

## 5. Universal Category System 8.2.1

`crates/mio-index/src/classify/ucs.rs` 里有一份从 UCS 派生、经本项目实测筛选的兜底词表。
UCS 是 Tim Nielsen、Justin Drury、Kai Paquin 等人发起的公有领域计划
（<https://www.universalcategorysystem.com/>），发布包本身**不随本项目分发**，也不在仓库里
（`/docs/UCS Release/` 已 gitignore）。随产物分发的只有派生的词表，由
`tools/import-ucs.py` 生成，筛选依据记录在 `tools/ucs-evidence.json`。
