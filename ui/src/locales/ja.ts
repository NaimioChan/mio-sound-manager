import type { MessageKey } from './en'

// Japanese.
//
// Register notes, because Japanese UI text fails in a specific way when it is
// translated literally from English:
//
// - Controls are noun phrases, not polite requests. 「してください」 belongs in a
//   dialog's body text, never on a button.
// - No trailing 「。」 on labels, buttons or menu items. Sentences in the empty
//   state and the index note keep it.
// - Width is the real constraint: a Japanese label is often shorter than its
//   English source but the glyphs are twice as wide, so the table headers stay
//   short (「長さ」「形式」 rather than 「再生時間」「サンプルレート」).
// - 「サンプル」 is the borrowed word for a sample and is what a producer expects;
//   「試料」 is a laboratory specimen and is wrong here.
// - 「ライブラリ」 for the library. 「音源」 would mean an instrument.

export const ja: Record<MessageKey, string> = {
  'app.subtitle': 'サンプル',

  'common.cancel': 'キャンセル',
  'common.save': '保存',
  'common.done': '完了',
  'common.clear': 'クリア',
  'common.dismiss': '閉じる',
  'common.collapse': '折りたたむ',
  'common.expand': '展開',

  'topbar.undo': '{label}を元に戻す (Ctrl+Z)',
  'topbar.redo': '{label}をやり直す (Ctrl+Shift+Z)',
  'topbar.settings': '設定 (Ctrl+,)',
  'topbar.settingsLabel': '設定',
  'topbar.undoLabel': '元に戻す',
  'topbar.redoLabel': 'やり直す',
  'topbar.addLibrary': 'ライブラリを追加 (Ctrl+O)',

  'search.placeholder': 'サンプルを検索…',
  'search.matches': '{count} 件',
  'search.matchesOne': '1 件',
  'search.addLibrary': 'ライブラリを追加',
  'search.favorites': 'お気に入り',

  'empty.backendError': 'バックエンドエラー',
  'empty.noLibrary': 'ライブラリが開かれていません',
  'empty.body':
    'サンプルのフォルダを指定してください。初回のインデックス作成では見つかったすべてのファイルを走査します。28 万ファイルで約 1 分、以降は数秒です。',
  'empty.open': 'ライブラリを開く…',

  'status.noDevice': 'オーディオデバイスを利用できません',
  'status.enginePeriod': 'エンジン周期 {ms} ms',
  'status.selected': '{count} 件選択',
  'status.loaded': '{loaded} / {matching} 件読み込み済み',
  'status.waveform': '波形 {ms} ms',

  'menu.play': '再生',
  'menu.addFavorite': 'お気に入りに追加',
  'menu.removeFavorite': 'お気に入りから削除',
  'menu.setCategory': 'カテゴリを設定…',
  'menu.clearCategory': 'カテゴリをクリア',
  'menu.reveal': 'エクスプローラーで表示',
  'menu.showInFolders': 'フォルダ内で表示',
  'menu.copyPath': 'パスをコピー',
  'menu.copyPaths': '{count} 件のパスをコピー',
  'menu.dragToDaw': 'DAW へドラッグ',
  'menu.dragToDawMany': '{count} 件を DAW へドラッグ',
  'menu.dragHint': '行をドラッグ',
  'menu.withCount': '{label} ({count})',

  'libMenu.switch': 'このライブラリに切り替え',
  'libMenu.reindex': '再インデックス',
  'libMenu.fullReindex': '完全に再構築',
  'libMenu.close': 'ライブラリを閉じる',

  'notice.pathsCopied': '{count} 件のパスをコピーしました',
  'notice.pathCopied': 'パスを 1 件コピーしました',
  'notice.undid': '{label}を元に戻しました',
  'notice.redid': '{label}をやり直しました',
  'notice.selected': '{count} 件選択',

  'edit.favorite': 'お気に入りに追加',
  'edit.unfavorite': 'お気に入りから削除',
  'edit.setCategory': 'カテゴリの設定',
  'edit.clearCategory': 'カテゴリのクリア',

  'index.indexing': 'ライブラリをインデックス中…',
  'index.reindexing': '再インデックス中…',
  'index.fullReindex': '完全に再構築中…',
  'index.walking': 'フォルダを走査中',
  'index.walkingEllipsis': 'フォルダを走査中…',
  'index.files': '{done} / {total} ファイル',
  'index.finishing': '仕上げ中…',
  'index.note':
    '完了するまでウィンドウはロックされます。28 万ファイルのライブラリで初回は約 1 分、以降は数秒です。',

  'col.name': '名前',
  'col.category': 'カテゴリ',
  'col.length': '長さ',
  'col.channels': 'ch',
  'col.rate': 'レート',
  'col.size': 'サイズ',
  'col.sortBy': '{column}で並べ替え',
  'row.addFavorite': 'お気に入りに追加',
  'row.removeFavorite': 'お気に入りから削除',
  'table.noMatch': '一致するサウンドはありません。',
  'table.count': '{shown} / {total}',
  'table.scrollForMore': 'スクロールで続きを表示',
  'table.searching': '検索中…',

  'side.categories': 'カテゴリ',
  'side.folders': 'フォルダ',
  'side.noLibrary': 'ライブラリが開かれていません',
  'side.favoritesOnly': 'お気に入りのみ表示',
  'side.favorites': 'お気に入り',
  'side.allSounds': 'すべてのサウンド',
  'side.allFolders': 'すべてのフォルダ',
  'side.resize': 'ドラッグで幅を変更',
  'side.uncategorized': '未分類',

  'dock.outputLevel': '出力レベル',
  'dock.mute': 'ミュート',
  'dock.unmute': 'ミュート解除',
  'dock.autoPlayOn': 'オートプレイ: オン',
  'dock.autoPlayOff': 'オートプレイ: オフ',
  'dock.autoPlayHint': '行をクリックすると試聴します (Alt+A)',
  'dock.autoPlayLabel': 'オートプレイ',
  'dock.pause': '一時停止 (Space)',
  'dock.resume': '再開 (Space)',
  'dock.pauseLabel': '一時停止',
  'dock.resumeLabel': '再開',
  'dock.stop': '停止して再生ヘッドを戻す',
  'dock.stopLabel': '停止',
  'dock.decoding': 'デコード中…',

  'settings.heading': '設定',
  'settings.subHeading': 'デバイス・ライブラリ・外観',
  'settings.outputDevice': '出力デバイス',
  'settings.device': 'デバイス',
  'settings.asioMissing':
    'このビルドには ASIO が含まれていません。./tools/build-app.sh --asio で再ビルドすると ASIO ドライバを選択できます。',
  'settings.library': 'ライブラリ',
  'settings.noLibrary': 'ライブラリが開かれていません。',
  'settings.soundCount': '{count} 件のサウンド',
  'settings.revealIndex': 'インデックスをエクスプローラーで表示',
  'settings.reveal': '表示',
  'settings.addLibrary': '別のライブラリを追加',
  'settings.openLibrary': 'ライブラリを開く…',
  'settings.rowsPerPage': '1 ページの行数',
  'settings.rowsPerPageHint':
    '1 回の検索で返す行数です。大きいほど次の取得までのスクロールが長くなります。',
  'settings.showCategory': 'カテゴリ列を表示',
  'settings.showCategoryHint': 'オフにすると一覧が詰まります。カテゴリでの絞り込みは左側で引き続き可能です。',
  'settings.behaviour': '動作',
  'settings.autoPlay': '選択時に自動再生',
  'settings.autoPlayHint': '行をクリックすると試聴します。ドックの AUTO PLAY と同じスイッチです。',
  'settings.infiniteScroll': '無限スクロール',
  'settings.infiniteScrollHint':
    '一覧が末尾に近づくと次のページを取得します。1 ページのみ表示する代わりです。',
  'settings.appearance': '外観',
  'settings.themeHint':
    'テーマは Siemens iX のトークン一式を塗り替えるため、各コントロール、パネル、波形が同じ配色に従います。',
  'settings.currentTheme': '現在: {name}',
  'settings.language': '言語',
  'settings.languageHint': 'すぐに反映され、このマシンに記憶されます。',

  'category.heading': 'カテゴリを設定',
  'category.oneSound': '1 件のサウンド',
  'category.nSounds': '{count} 件のサウンド',
  'category.label': 'カテゴリ',
  'category.placeholder': '例: 武器',
  'category.subLabel': 'サブカテゴリ (任意)',
  'category.subPlaceholder': '例: 銃器',
  'category.existing': '既存: {names}',
  'category.pickExisting': '既存から選択',
  'category.subcategoriesOf': '{name} のサブカテゴリ',
  'category.clear': 'カテゴリをクリア',

  'theme.classicDark': 'クラシック ダーク',
  'theme.classicDarkHint': 'Siemens iX 標準',
  'theme.classicLight': 'クラシック ライト',
  'theme.classicLightHint': 'Siemens iX 反転',
  'theme.studioDark': 'スタジオ ダーク',
  'theme.studioDarkHint': 'グラファイトと紫、長時間作業向け',
  'theme.studioLight': 'スタジオ ライト',
  'theme.studioLightHint': '紙白と藍、日中向け',
  'theme.amber': 'アンバー コンソール',
  'theme.amberHint': 'ほぼ黒に琥珀色の線、暗い部屋向け',
  'theme.frost': 'フロスト',
  'theme.frostHint': '寒色の青灰、まぶしさを抑える',

  'channel.mono': 'モノラル',
  'channel.stereo': 'ステレオ',
  'channel.n': '{count}ch'
}
