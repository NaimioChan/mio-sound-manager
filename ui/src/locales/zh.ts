import type { MessageKey } from './en'

// Simplified Chinese.
//
// Register rules this file follows, because a sample browser's labels are mostly
// nouns and the usual machine-translation habits read wrong here:
//
// - Controls are verbs or nouns, no 「请」, no trailing 「。」 on a button.
// - 「采样」 is the sample in a sample library; 「样本」 is a statistical sample and
//   is wrong here. 「声音」 is what a row is.
// - 「音色」 is a synth patch, not a sample. Do not use it for the library.
// - Menu items keep the ellipsis (「…」) where a dialog follows, matching the
//   English: it is the signal that a click opens something.
// - Numbers and units stay in Latin digits: 128 Hz, 24 ch.

export const zh: Record<MessageKey, string> = {
  'common.cancel': '取消',
  'common.save': '保存',
  'common.done': '完成',
  'common.clear': '清除',
  'common.dismiss': '关闭',
  'common.collapse': '折叠',
  'common.expand': '展开',

  'topbar.undo': '撤销{label}（Ctrl+Z）',
  'topbar.redo': '重做{label}（Ctrl+Shift+Z）',
  'topbar.settings': '设置（Ctrl+,）',
  'topbar.settingsLabel': '设置',
  'topbar.undoLabel': '撤销',
  'topbar.redoLabel': '重做',
  'topbar.addLibrary': '添加音色库（Ctrl+O）',

  'search.placeholder': '搜索采样…',
  'search.matches': '{count} 个匹配',
  'search.matchesOne': '1 个匹配',
  'search.addLibrary': '添加库',
  'search.favorites': '收藏',

  'empty.backendError': '后端错误',
  'empty.noLibrary': '未打开任何库',
  'empty.body':
    '指向一个存放采样的文件夹。首次索引会扫描其中每个文件，28 万个文件约需一分钟，之后只需几秒。',
  'empty.open': '打开库…',

  'status.noDevice': '音频设备不可用',
  'status.enginePeriod': '引擎周期 {ms} ms',
  'status.selected': '已选 {count} 项',
  'status.loaded': '已加载 {loaded} / {matching}',
  'status.waveform': '波形 {ms} ms',

  'menu.play': '播放',
  'menu.addFavorite': '加入收藏',
  'menu.removeFavorite': '取消收藏',
  'menu.setCategory': '设置分类…',
  'menu.clearCategory': '清除分类',
  'menu.reveal': '在文件管理器中显示',
  'menu.showInFolders': '在文件夹中定位',
  'menu.copyPath': '复制路径',
  'menu.copyPaths': '复制 {count} 个路径',
  'menu.withCount': '{label}（{count}）',

  'folderMenu.reveal': '在文件管理器中显示',
  'folderMenu.filter': '只看此文件夹',
  'folderMenu.copyPath': '复制文件夹路径',

  'libMenu.switch': '切换到此库',
  'libMenu.reindex': '重新索引',
  'libMenu.fullReindex': '完整重建索引',
  'libMenu.close': '关闭库',

  'notice.pathsCopied': '已复制 {count} 个路径',
  'notice.pathCopied': '已复制 1 个路径',
  'notice.undid': '已撤销{label}',
  'notice.redid': '已重做{label}',
  'notice.selected': '已选 {count} 项',

  'edit.favorite': '加入收藏',
  'edit.unfavorite': '取消收藏',
  'edit.setCategory': '设置分类',
  'edit.clearCategory': '清除分类',

  'index.indexing': '正在索引音色库…',
  'index.reindexing': '正在重新索引…',
  'index.fullReindex': '正在完整重建索引…',
  'index.walking': '正在遍历文件夹',
  'index.walkingEllipsis': '正在遍历文件夹…',
  'index.files': '{done} / {total} 个文件',
  'index.finishing': '正在收尾…',
  'index.note': '此过程完成前窗口保持锁定。28 万个文件的库首次约需一分钟，之后只需几秒。',

  'col.name': '名称',
  'col.category': '分类',
  'col.length': '时长',
  'col.bpm': 'BPM',
  'col.channels': '声道',
  'col.rate': '采样率',
  'col.size': '大小',
  'col.sortBy': '按{column}排序',
  'row.addFavorite': '加入收藏',
  'row.removeFavorite': '取消收藏',
  'table.noMatch': '没有匹配的声音。',
  'table.count': '{shown} / {total}',
  'table.scrollForMore': '继续滚动加载',
  'table.searching': '搜索中…',

  'side.categories': '分类',
  'side.folders': '文件夹',
  'side.noLibrary': '未打开任何库',
  'side.favoritesOnly': '只显示收藏',
  'side.favorites': '收藏',
  'side.allSounds': '全部声音',
  'side.allFolders': '全部文件夹',
  'side.resize': '拖动调整宽度',
  'side.uncategorized': '未分类',
  'side.tagsHint': 'Ctrl+点击可多选',

  'dock.outputLevel': '输出音量',
  'dock.mute': '静音',
  'dock.unmute': '取消静音',
  'dock.autoPlayOn': '自动播放：开',
  'dock.autoPlayOff': '自动播放：关',
  'dock.autoPlayHint': '点击行即试听（Alt+A）',
  'dock.autoPlayLabel': '自动播放',
  'dock.pause': '暂停（空格）',
  'dock.resume': '继续（空格）',
  'dock.pauseLabel': '暂停',
  'dock.resumeLabel': '继续',
  'dock.stop': '停止并清除播放头',
  'dock.stopLabel': '停止',
  'dock.decoding': '正在解码…',

  'settings.heading': '设置',
  'settings.outputDevice': '输出设备',
  'settings.device': '设备',
  'settings.asioMissing': '此构建不含 ASIO 后端。',
  'settings.library': '音色库',
  'settings.noLibrary': '未打开任何库。',
  'settings.soundCount': '{count} 个声音',
  'settings.revealIndex': '在文件管理器中显示索引',
  'settings.reveal': '显示',
  'settings.addLibrary': '添加另一个库',
  'settings.openLibrary': '打开库…',
  'settings.rowsPerPage': '每页行数',
  'settings.showCategory': '显示分类列',
  'settings.behaviour': '行为',
  'settings.autoPlay': '选中即播放',
  'settings.infiniteScroll': '无限滚动',
  'settings.appearance': '外观',
  'settings.zoom': '界面缩放',
  'settings.language': '语言',

  'category.heading': '设置分类',
  'category.selected': '已选 {count} 项',
  'category.label': '分类',
  'category.placeholder': '例如 武器',
  'category.subLabel': '子分类（可选）',
  'category.subPlaceholder': '例如 枪械',
  'category.existing': '已有：{names}',
  'category.pickExisting': '选择已有分类',
  'category.subcategoriesOf': '{name} 的子分类',
  'category.clear': '清除分类',

  'theme.classicDark': '经典深色',
  'theme.classicLight': '经典浅色',
  'theme.studioDark': '工作室深色',
  'theme.studioLight': '工作室浅色',
  'theme.amber': '琥珀终端',
  'theme.frost': '霜',

  'channel.mono': '单声道',
  'channel.stereo': '立体声',
  'channel.n': '{count} 声道'
}
