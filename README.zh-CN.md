# Mio Sound Manager

一款面向编曲与音效制作的 Windows 本地音频采样管理器，基于 Rust 和 Tauri 2。

**[English](README.md) · [日本語](README.ja.md)**

![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)
![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-green)
![Version](https://img.shields.io/badge/version-0.3.1-orange)

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri%202-24C8D8?logo=tauri&logoColor=white)
![SolidJS](https://img.shields.io/badge/SolidJS-2C4F7C?logo=solid&logoColor=white)
![Siemens iX](https://img.shields.io/badge/Siemens%20iX-009999)
![SQLite](https://img.shields.io/badge/SQLite%20+%20FTS5-003B57?logo=sqlite&logoColor=white)
![symphonia](https://img.shields.io/badge/symphonia-gray)
![WASAPI / ASIO](https://img.shields.io/badge/WASAPI%20%2F%20ASIO-gray)

---

## 核心体验

**够轻**  
安装包仅约 5 MB，不依赖后台服务或额外运行时。内存占用低，不影响正在运行的 DAW。

**够快**  
索引几十万个本地采样只需约一分钟，增量更新通常在几秒内完成。搜索、滚动、跳转均无明显等待。支持 WASAPI 和 ASIO 两种音频设备协议，获得极低延迟的回放体验。

**够全面**  
支持 WAV、FLAC、MP3、AIFF、M4A、OGG 等主流格式，读取 ID3、BWF、iXML 等内嵌标签，自动按 UCS 分类与常用制作人分类打标，可以框选波形片段直接拖入 DAW。

---

## 功能介绍

### 低延迟试听与播放
- **双音频输出**：默认共享模式开箱即用；支持在设置中选择已安装的 ASIO 驱动并调整缓冲大小。
- **即点即播**：支持选中行自动试听（Auto Play），短采样瞬间响应，长音频也可以在任意位置直接跳转播放。
- **干净的起音**：自动处理 MP3、M4A/AAC 等格式开头的编码静音，试听鼓组和瞬态采样时不会拖泥带水。
- **常用播控**：独立的输出音量与静音记忆，支持空格暂停/继续、`L` 键循环播放。

### 波形预览与片段拖出
- **清晰波形**：单声道与立体声分轨显示，点击波形任意位置即可定位播放。
- **区域框选与循环**：在波形上拖拽即可框选片段，支持拖动左右边界微调；开启循环后，有框选时只循环选区，没有框选时循环整条音频，按 `Esc` 清除框选。
- **拖入 DAW**：从列表行可直接拖出原文件；从波形顶部拖出时，若有框选会自动导出选区为 WAV 文件并拖入宿主或桌面。

### 检索、分类与多库管理
- **同时管理多个采样库**：可添加多个本地或移动硬盘上的采样文件夹，自动记住上次打开的库；外接硬盘未连接时保留记录，插回即可使用。
- **快速搜索与组合筛选**：输入关键词即可搜索文件名、路径、描述和嵌入标签，并支持按收藏、分类、文件夹、声道数、采样率、BPM 和封面筛选。
- **自动分类与手动整理**：内置 UCS 8.2.1 标准音效分类与 22 个常用制作人分类，自动识别文件名中的标准 CatID 与 BPM；支持批量收藏和手动修改分类（重新索引不会覆盖手动分类），并支持 50 步撤销与重做。

### 详情面板、标签与封面
- **右侧详情面板**：集中查看音频时长、声道、采样率、BPM、大小、路径以及分类归属，面板支持折叠和拖动调宽。
- **完整内嵌标签**：支持查看 ID3、Vorbis、M4A、BWF `bext`、iXML、LIST INFO 等常见音频元数据。
- **封面展示与同封面查找**：支持读取音频内嵌封面和同目录封面图，列表可开启封面缩略图列；在详情面板中点击封面或任意属性，可以快速筛选出同一套素材包或同规格的采样。

### 外观与习惯定制
- **7 套内置配色**：Windows 深色 / 浅色、工作室深色 / 浅色、琥珀终端、霜、东京夜。
- **3 种界面语言**：简体中文、English、日本語，支持界面缩放调节。
- **自定义列表**：12 个信息列支持自由调整宽度、右键表头或在设置中控制显示与隐藏。

---

## 常用快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Space` | 暂停 / 继续 |
| `Alt + A` | 开关自动播放 |
| `L` | 开关循环播放 |
| `↑` / `↓` / `PageUp` / `PageDown` / `Home` / `End` | 列表移动与试听 |
| `Ctrl + A` | 全选当前结果 |
| `Ctrl + Z` | 撤销收藏或分类修改 |
| `Ctrl + Shift + Z` / `Ctrl + Y` | 重做 |
| `Ctrl + O` | 添加采样库 |
| `Ctrl + ,` | 打开设置 |
| `Esc` | 清除波形框选 / 关闭菜单与弹窗 |

---

## 支持格式

- **音频格式**：`.wav` `.aif` `.aiff` `.flac` `.mp3` `.ogg` `.m4a` `.aac` `.opus`
- **元数据与封面**：ID3、Vorbis Comments、M4A Atoms、BWF `bext`、iXML、RIFF `LIST INFO`；内嵌封面与同目录图片

---

## 从源码构建

### 环境要求
- Windows 10 / 11（64 位）
- Rust（Edition 2024）与 Node.js / npm
- 如需包含 ASIO 支持：Steinberg ASIO SDK 2.3（解压至 `%LOCALAPPDATA%\mio\asio_sdk`）和 `libclang.dll`（设置 `LIBCLANG_PATH`）

### 构建命令

```bash
# 开发构建（仅 WASAPI，不需要 ASIO SDK）
./tools/build-app.sh --no-asio

# 完整构建（WASAPI + ASIO）
./tools/build-app.sh

# 打包 Windows 安装程序
./ui/node_modules/.bin/tauri build --features asio --bundles nsis
```

---

## 开源协议

本项目基于 **GNU AGPL v3**（`AGPL-3.0-or-later`）开源，详见 [LICENSE](LICENSE)；第三方组件与授权说明见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
