// The icons the UI draws, as SVG files in this repository.
//
// These are copied out of `@siemens/ix-icons` (MIT, Siemens AG) into
// `src/assets/icons/`, and the copies are the ones that ship. That is deliberate:
//
// - `ix-icon` given a *name* builds the URL `svg/<name>.svg` relative to the
//   Stencil asset path and fetches it. That directory is not in this bundle, so
//   every name-based icon failed and fell back to iX's error symbol.
// - Files in the repo are what the offline requirement asks for: no network
//   request at any point, no dependency on a package's internal asset layout, and
//   a diff that shows when an icon actually changes.
//
// **The data URL must carry the SVG text unencoded.** iX runs the string through
// `decodeURI` (not `decodeURIComponent`), which leaves reserved characters like
// `%3D` untouched — so an `encodeURIComponent`-encoded SVG parses into an `<svg>`
// whose attributes are literally `width%3d"512px"`, every path loses its `d`, and
// the icon renders as an empty box. iX's own data URLs are raw text for this
// reason. These files contain no `#` or `%`, so a raw URL is safe.
//
// Adding an icon: copy it from `node_modules/@siemens/ix-icons/svg/<name>.svg`
// into `src/assets/icons/` and import it below. A name with no entry here is a
// compile error rather than a blank square.

import cogwheel from './assets/icons/cogwheel.svg?raw'
import database from './assets/icons/database.svg?raw'
import error from './assets/icons/error.svg?raw'
import folderOpen from './assets/icons/folder-open.svg?raw'
import loop from './assets/icons/loop.svg?raw'
import pause from './assets/icons/pause.svg?raw'
import play from './assets/icons/play.svg?raw'
import redo from './assets/icons/redo.svg?raw'
import soundLoud from './assets/icons/sound-loud.svg?raw'
import soundMute from './assets/icons/sound-mute.svg?raw'
import stop from './assets/icons/stop.svg?raw'
import undo from './assets/icons/undo.svg?raw'

/**
 * `ix-icon` accepts either a name or a data URL. The `data:image/svg+xml` prefix is
 * what makes it parse the string inline instead of fetching it.
 *
 * The XML declaration is stripped: iX parses the payload as HTML, where
 * `<?xml …?>` is not a declaration but a bogus comment, and half the icon files
 * start with one.
 */
const asDataUrl = (svg: string): string =>
  `data:image/svg+xml;utf8,${svg.replace(/<\?xml[^>]*\?>/g, '').trim()}`

/** The icons this app draws. */
export const ICON = {
  play: asDataUrl(play),
  pause: asDataUrl(pause),
  stop: asDataUrl(stop),
  settings: asDataUrl(cogwheel),
  folderOpen: asDataUrl(folderOpen),
  sound: asDataUrl(soundLoud),
  mute: asDataUrl(soundMute),
  loop: asDataUrl(loop),
  undo: asDataUrl(undo),
  redo: asDataUrl(redo),
  database: asDataUrl(database),
  error: asDataUrl(error)
} as const

export type IconName = (typeof ICON)[keyof typeof ICON]
