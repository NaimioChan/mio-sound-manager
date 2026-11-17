// iX icon helpers.
//
// iX ships ~1500 SVG icons as data URLs. `ix-icon` resolves a kebab-case name
// against its own registry, but the *props* the components take (`icon="play"` on
// a button, `icon="star"` on a pill) go through the same lookup, so the names
// below are the only ones used in this app. Keeping them in one place means a
// typo is a compile error instead of a blank 16px square.

export const ICON = {
  play: 'play',
  pause: 'pause',
  stop: 'stop',
  search: 'search',
  folder: 'folder',
  folderOpen: 'folder-open',
  star: 'star',
  starFilled: 'star-filled',
  chevronRight: 'chevron-right',
  chevronDown: 'chevron-down',
  settings: 'cogwheel',
  sound: 'sound-loud',
  mute: 'sound-mute',
  palette: 'palette',
  undo: 'undo',
  redo: 'redo',
  plus: 'plus',
  close: 'close',
  music: 'music-note',
  database: 'database',
  refresh: 'refresh',
  history: 'history',
  check: 'check',
  bulb: 'bulb',
  sun: 'sun',
  moon: 'moon',
  sort: 'sort',
  filter: 'filter',
  expand: 'expand-all',
  collapse: 'collapse-all',
  more: 'more-menu',
  info: 'info',
  warning: 'warning',
  error: 'error'
} as const

export type IconName = (typeof ICON)[keyof typeof ICON]
