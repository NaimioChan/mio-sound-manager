// The English dictionary. It is the source of truth for the key set: every other
// locale is typed as `Record<MessageKey, string>`, so a missing or misspelled key
// in a translation is a compile error rather than a blank label at runtime.
//
// Placeholders are `{name}` and are substituted by `t()`. A translation may reorder
// them; it may not drop one that the English text has, and `tools/check-i18n.mjs`
// fails when it does.
//
// What is *not* in here: product name, category names from the index, file paths,
// sample rates and other data. Those are values, not messages.

export const en = {
  'common.cancel': 'Cancel',
  'common.save': 'Save',
  'common.done': 'Done',
  'common.clear': 'Clear',
  'common.dismiss': 'Dismiss',
  'common.collapse': 'Collapse',
  'common.expand': 'Expand',

  // ---------------------------------------------------------------- top bar
  'topbar.undo': 'Undo {label} (Ctrl+Z)',
  'topbar.redo': 'Redo {label} (Ctrl+Shift+Z)',
  'topbar.settings': 'Settings (Ctrl+,)',
  'topbar.settingsLabel': 'Settings',
  'topbar.undoLabel': 'Undo',
  'topbar.redoLabel': 'Redo',
  'topbar.addLibrary': 'Add a library (Ctrl+O)',

  // ------------------------------------------------------------------ search
  'search.placeholder': 'Search samples…',
  'search.matches': '{count} matches',
  'search.matchesOne': '1 match',
  'search.addLibrary': 'Add library',
  'search.favorites': 'Favorites',

  // ------------------------------------------------------------- empty state
  'empty.backendError': 'Backend error',
  'empty.noLibrary': 'No library open',
  'empty.body':
    'Point Mio at a folder of samples. The first pass indexes every file it finds, which takes about a minute for 280k files and a few seconds after that.',
  'empty.open': 'Open library…',

  // --------------------------------------------------------------- status bar
  'status.noDevice': 'audio device unavailable',
  'status.enginePeriod': 'engine period {ms} ms',
  'status.selected': '{count} selected',
  'status.loaded': '{loaded} of {matching} loaded',
  'status.waveform': 'waveform {ms} ms',

  // -------------------------------------------------------------- row menu
  'menu.play': 'Play',
  'menu.addFavorite': 'Add to Favorites',
  'menu.removeFavorite': 'Remove from Favorites',
  'menu.setCategory': 'Set Category…',
  'menu.clearCategory': 'Clear Category',
  'menu.reveal': 'Reveal in File Manager',
  'menu.showInFolders': 'Show in Folders',
  'menu.copyPath': 'Copy Path',
  'menu.copyPaths': 'Copy {count} Paths',
  'menu.withCount': '{label} ({count})',

  // ------------------------------------------------------------- folder menu
  'folderMenu.reveal': 'Reveal in File Manager',
  'folderMenu.filter': 'Filter this folder',
  'folderMenu.copyPath': 'Copy Folder Path',

  // ---------------------------------------------------------- library menu
  'libMenu.switch': 'Switch to this library',
  'libMenu.reindex': 'Reindex',
  'libMenu.fullReindex': 'Full reindex',
  'libMenu.close': 'Close library',

  // ---------------------------------------------------------------- notices
  'notice.pathsCopied': '{count} paths copied',
  'notice.pathCopied': '1 path copied',
  'notice.undid': 'Undid {label}',
  'notice.redid': 'Redid {label}',
  'notice.selected': '{count} selected',

  // What an undoable edit was. Title case: these appear as tooltips and after
  // "Undid"/"Redid", where the sentence form is lowercased by `editKindName`.
  'edit.favorite': 'Add to Favorites',
  'edit.unfavorite': 'Remove from Favorites',
  'edit.setCategory': 'Set Category',
  'edit.clearCategory': 'Clear Category',

  // ------------------------------------------------------------- index pass
  'index.indexing': 'Indexing library…',
  'index.reindexing': 'Reindexing…',
  'index.fullReindex': 'Full reindex…',
  'index.walking': 'walking the folder',
  'index.walkingEllipsis': 'walking the folder…',
  'index.files': '{done} / {total} files',
  'index.finishing': 'finishing up…',
  'index.note':
    'The window stays locked until this finishes. A 280k-file library takes about a minute the first time and a few seconds after that.',

  // ------------------------------------------------------------ table header
  'col.name': 'Name',
  'col.category': 'Category',
  'col.length': 'Length',
  'col.bpm': 'BPM',
  'col.channels': 'Ch',
  'col.rate': 'Rate',
  'col.size': 'Size',
  'col.sortBy': 'Sort by {column}',
  'row.addFavorite': 'Add to favorites',
  'row.removeFavorite': 'Remove from favorites',
  'table.noMatch': 'No sounds match.',
  'table.count': '{shown} of {total}',
  'table.scrollForMore': 'scroll for more',
  'table.searching': 'searching…',

  // ----------------------------------------------------------------- sidebar
  'side.categories': 'Categories',
  'side.folders': 'Folders',
  'side.noLibrary': 'no library open',
  'side.favoritesOnly': 'Show favorites only',
  'side.favorites': 'Favorites',
  'side.allSounds': 'All sounds',
  'side.allFolders': 'All folders',
  'side.resize': 'Drag to resize',
  'side.uncategorized': 'Uncategorized',
  'side.tagsHint': 'Ctrl+click to combine',

  // -------------------------------------------------------------------- dock
  'dock.outputLevel': 'Output level',
  'dock.mute': 'Mute',
  'dock.unmute': 'Unmute',
  'dock.autoPlayOn': 'Auto-play on',
  'dock.autoPlayOff': 'Auto-play off',
  'dock.autoPlayHint': 'clicking a row auditions it (Alt+A)',
  'dock.autoPlayLabel': 'AUTO PLAY',
  'dock.pause': 'Pause (Space)',
  'dock.resume': 'Resume (Space)',
  'dock.pauseLabel': 'Pause',
  'dock.resumeLabel': 'Resume',
  'dock.stop': 'Stop and clear the playhead',
  'dock.stopLabel': 'Stop',
  'dock.decoding': 'decoding…',

  // ---------------------------------------------------------------- settings
  'settings.heading': 'Settings',
  'settings.outputDevice': 'Output device',
  'settings.device': 'Device',
  'settings.asioMissing': 'This build has no ASIO backend.',
  'settings.library': 'Library',
  'settings.noLibrary': 'No library is open.',
  'settings.soundCount': '{count} sounds',
  'settings.revealIndex': 'Reveal the index',
  'settings.reveal': 'Reveal',
  'settings.addLibrary': 'Add another library',
  'settings.openLibrary': 'Open library…',
  'settings.rowsPerPage': 'Rows per page',
  'settings.showCategory': 'Show the category column',
  'settings.behaviour': 'Behaviour',
  'settings.autoPlay': 'Auto-play on select',
  'settings.infiniteScroll': 'Infinite scroll',
  'settings.appearance': 'Appearance',
  'settings.zoom': 'Interface scale',
  'settings.language': 'Language',

  // ---------------------------------------------------------- category dialog
  'category.heading': 'Set category',
  'category.selected': '{count} selected',
  'category.label': 'Category',
  'category.placeholder': 'e.g. Weapons',
  'category.subLabel': 'Subcategory (optional)',
  'category.subPlaceholder': 'e.g. Firearms',
  'category.existing': 'Existing: {names}',
  'category.pickExisting': 'Pick an existing one',
  'category.subcategoriesOf': 'Subcategories of {name}',
  'category.clear': 'Clear category',

  // ----------------------------------------------------------------- themes
  'theme.classicDark': 'Classic Dark',
  'theme.classicLight': 'Classic Light',
  'theme.studioDark': 'Studio Dark',
  'theme.studioLight': 'Studio Light',
  'theme.amber': 'Amber Console',
  'theme.frost': 'Frost',

  // ------------------------------------------------------- channel labels
  'channel.mono': 'Mono',
  'channel.stereo': 'Stereo',
  'channel.n': '{count}ch'
} as const

export type MessageKey = keyof typeof en
