# Mio Sound Manager

作曲とサウンドデザイン向けの Windows ローカル音声サンプル管理ツール。Rust と Tauri 2 ベース。

**[English](README.md) · [简体中文](README.zh-CN.md)**

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

## 特徴

**軽量**  
インストーラーは約 5 MB。バックグラウンドサービスや追加ランタイム不要。メモリ消費が少なく、起動中の DAW に影響を与えない。

**高速**  
数十万件のサンプルのインデックス構築は約 1 分。差分更新は数秒で完了。検索・スクロール・フィルタリングもストレスなく動作する。WASAPI と ASIO の両方に対応し、低レイテンシな試聴を実現。

**多機能**  
WAV・FLAC・MP3・AIFF・M4A・OGG 等の主要フォーマットに対応。ID3・BWF・iXML 等の埋め込みタグを読み込む。UCS およびプロデューサー向けカテゴリで自動分類。波形上でリージョンを選択し、そのまま DAW にドラッグ＆ドロップ。

---

## 機能

### 試聴と再生
- **WASAPI / ASIO 対応** — デフォルトは WASAPI 共有モード（DAW と同時使用可）。設定から ASIO ドライバーに切り替え可能。
- **自動再生** — 行を選択するだけで即座に試聴（`Alt+A` でトグル）。
- **クリーンなアタック** — MP3・M4A/AAC のエンコーダーディレイとプライミングフレームを自動でトリミング。ドラムやトランジェントの試聴時に先頭が欠けない。
- **再生コントロール** — 音量ノブとミュート記憶、`Space` で一時停止/再開、`L` でループ切り替え。

### 波形表示とリージョン書き出し
- **チャンネル別波形** — モノラル・ステレオを個別レーンで表示。クリックで任意の位置にシーク。
- **リージョン選択とループ** — ドラッグで範囲選択、端をドラッグして微調整。ループ時は選択範囲のみ繰り返し再生。選択なしの場合はファイル全体をループ。`Esc` で選択を解除。
- **DAW へのドラッグ** — 行からオリジナルファイルをドラッグ。波形ハンドルからドラッグすると、選択範囲を 32-bit float WAV として書き出してドロップ（選択なしの場合は元ファイル）。

### 検索・分類・ライブラリ管理
- **複数ライブラリ対応** — ローカルや外付けドライブのフォルダを複数追加可能。セッションはリスタート後も保持。外付けドライブが未接続でもエントリは保持される。
- **全文検索とフィルタリング** — ファイル名・パス・説明・埋め込みタグを検索。お気に入り・カテゴリ・フォルダ・チャンネル数・サンプルレート・BPM・カバーアートで絞り込み可能。
- **自動分類と手動編集** — UCS 8.2.1（CatID 認識、82 カテゴリ、753 サブカテゴリ）とプロデューサー向け 22 カテゴリで自動分類。ファイル名から BPM を自動解析。手動設定はインデックス再構築後も維持。50 ステップのアンドゥ/リドゥに対応。

### インスペクタ・タグ・カバーアート
- **右側インスペクタパネル** — 再生時間・チャンネル・サンプルレート・BPM・サイズ・パス・カテゴリを一覧表示。折りたたみ・リサイズ可能。
- **生タグ表示** — ID3・Vorbis Comments・M4A Atoms・BWF `bext`・iXML・LIST INFO をソース別にグループ表示。
- **カバーアート** — 埋め込みアートワークとサイドカー画像を読み込み。リスト列にサムネイル表示可能。任意の属性をクリックしてすぐに絞り込み・検索が可能。

### 外観とカスタマイズ
- **7 種類のテーマ** — Windows ダーク/ライト、スタジオ ダーク/ライト、アンバーコンソール、フロスト、東京ナイト。
- **3 言語対応** — English、简体中文、日本語。UI スケール調整可能。
- **カスタマイズ可能な列** — 12 列、幅のドラッグ変更、右クリックメニューまたは設定から表示/非表示の切り替え。

---

## キーボードショートカット

| ショートカット | 操作 |
| --- | --- |
| `Space` | 一時停止 / 再開 |
| `Alt + A` | 自動再生の切り替え |
| `L` | ループの切り替え |
| `↑` / `↓` / `PageUp` / `PageDown` / `Home` / `End` | リスト移動と試聴 |
| `Ctrl + A` | 現在の結果をすべて選択 |
| `Ctrl + Z` | お気に入りまたはカテゴリ編集をアンドゥ |
| `Ctrl + Shift + Z` / `Ctrl + Y` | リドゥ |
| `Ctrl + O` | サンプルライブラリを追加 |
| `Ctrl + ,` | 設定を開く |
| `Esc` | 波形リージョンをクリア / メニュー・ダイアログを閉じる |

---

## 対応フォーマット

- **音声：** `.wav` `.aif` `.aiff` `.flac` `.mp3` `.ogg` `.m4a` `.aac` `.opus`
- **メタデータとカバーアート：** ID3、Vorbis Comments、M4A Atoms、BWF `bext`、iXML、RIFF `LIST INFO`；埋め込みおよびサイドカー画像

---

## ソースからのビルド

### 必要な環境
- Windows 10 / 11（64 ビット）
- Rust（Edition 2024）と Node.js / npm
- ASIO サポートを含める場合：Steinberg ASIO SDK 2.3（`%LOCALAPPDATA%\mio\asio_sdk` に展開）と `libclang.dll`（`LIBCLANG_PATH` を設定）

### ビルドコマンド

```bash
# 開発ビルド（WASAPI のみ、ASIO SDK 不要）
./tools/build-app.sh --no-asio

# フルビルド（WASAPI + ASIO）
./tools/build-app.sh

# Windows インストーラーのパッケージング
./ui/node_modules/.bin/tauri build --features asio --bundles nsis
```

---

## ライセンス

本プロジェクトは **GNU AGPL v3**（`AGPL-3.0-or-later`）のもとで公開されています。詳細は [LICENSE](LICENSE) を参照。サードパーティコンポーネントのライセンス情報は [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) を参照。
