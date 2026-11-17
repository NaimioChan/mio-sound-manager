// The IPC contract, in one place.
//
// Every type here mirrors a `#[derive(Serialize)]` struct in
// `crates/mio-app/src/commands.rs`. They are hand-written rather than
// generated because the surface is small and a generator would need a build step
// that runs before the webview bundles — the one thing that must not fail on a
// machine without Rust installed.
//
// Field names are camelCase on the wire. Rust does the rename through
// `#[serde(rename_all = "camelCase")]`, so never send a snake_case payload.

import { Channel, invoke } from '@tauri-apps/api/core'

export interface LibraryStatus {
  root: string
  /** Folder name only, for the sidebar header. The path is the tooltip. */
  name: string
  count: number
  indexing: boolean
  indexed: number
  total: number
  /** Whether the window is showing this library. */
  active: boolean
}

export interface AppStatus {
  audioAvailable: boolean
  audioError: string | null
  deviceName: string | null
  /** "WASAPI" or "ASIO". The status bar shows it next to the period. */
  backend: string
  sampleRate: number | null
  channels: number | null
  sampleFormat: string | null
  endpointFrames: number | null
  enginePeriodMs: number | null
  library: LibraryStatus | null
}

/** One entry in the output-device picker. */
export interface AudioDevice {
  backend: string
  /** The value sent back to `setAudioDevice`. */
  name: string
  label: string
  channels: number | null
  /** ASIO only: the driver's preferred callback buffer size. */
  bufferFrames: number | null
  default: boolean
  /** False when the device exists but cannot be opened (an unplugged interface). */
  available: boolean
}

export interface PlaybackPosition {
  positionS: number
  durationS: number
  playing: boolean
  /** Loaded but frozen: the playhead holds still and Space resumes it. */
  paused: boolean
  finished: boolean
}

export interface Waveform {
  peaks: number[]
  durationS: number
  sampleRate: number
  channels: number
  decodeMs: number
}

export interface SoundRow {
  path: string
  filename: string
  durationS: number | null
  durationLabel: string
  channels: number | null
  sampleRate: number | null
  filesize: number | null
  category: string
  subcategory: string | null
  confidence: number
  matchedTerms: string[]
  favorite: boolean
  categoryManual: boolean
  folder: string | null
  /** Tempo stated in the file name; `null` when the name states none. */
  bpm: number | null
}

export interface FolderNode {
  name: string
  path: string
  count: number
  children: FolderNode[]
}

/** A right-click on a folder row in the rail. */
export interface FolderMenuRequest {
  path: string
  /** Last path segment, for the menu's own label. */
  name: string
  x: number
  y: number
}

export interface Facet {
  category: string
  subcategory: string | null
  count: number
}

export interface Sidebar {
  facets: Facet[]
  favorites: number
  total: number
}

/** What an undoable edit was. The words for these live in `locales/`. */
export type EditKind = 'favorite' | 'unfavorite' | 'setCategory' | 'clearCategory'

export interface HistoryState {
  undoKind: EditKind | null
  redoKind: EditKind | null
}

/** One tick from a blocking index pass. */
export interface IndexProgress {
  done: number
  total: number
}

export interface SearchRequest {
  query?: string
  maxDuration?: number
  channels?: number
  /** Several categories are OR-ed; empty means no category filter. */
  categories?: string[]
  /** Intersects with `categories`, which is what the rail's lit rows mean. */
  subcategories?: string[]
  folder?: string
  minConfidence?: number
  favoriteOnly?: boolean
  sortBy?: string
  sortDir?: 'asc' | 'desc'
  limit?: number
  offset?: number
}

export const api = {
  status: () => invoke<AppStatus>('get_status'),

  /** Every output device, both backends. Read when the settings dialog opens. */
  audioDevices: () => invoke<AudioDevice[]>('list_audio_devices'),

  /** Whether this build can open ASIO drivers at all. */
  asioCompiled: () => invoke<boolean>('asio_compiled'),

  /**
   * Switch the output device.
   *
   * Returns the new status rather than throwing on a device that will not open:
   * the dialog shows the reason next to the picker, and the app stays usable.
   */
  setAudioDevice: (backend: string, device: string | null, asioBufferFrames?: number | null) =>
    invoke<AppStatus>('set_audio_device', {
      backend,
      device,
      asioBufferFrames: asioBufferFrames ?? null
    }),

  /**
   * Start playing `path`.
   *
   * Resolves once audio is published to the device, which is what the playhead
   * wants: the position command reports frames the render thread has written,
   * so a playhead drawn before this resolves would sit at zero.
   */
  play: (path: string, startAt?: number) =>
    invoke<void>('play', { path, startAt: startAt ?? null }),

  stop: () => invoke<void>('stop'),

  /** Freeze the loaded clip in place. Resuming does not re-decode or re-seek. */
  pause: () => invoke<void>('pause'),

  resume: () => invoke<void>('resume'),

  position: () => invoke<PlaybackPosition>('get_playback_position'),

  /**
   * Output gain. `1.0` is unity, `0.0` silence; the engine clamps to `0..4`.
   *
   * One atomic store on the render thread's side, so it takes effect on the
   * playing clip without a re-publish and without a re-decode.
   */
  setGain: (gain: number) => invoke<number>('set_gain', { gain }),

  gain: () => invoke<number>('get_gain'),

  waveform: (path: string, buckets?: number) =>
    invoke<Waveform>('get_waveform', { path, buckets: buckets ?? null }),

  /**
   * Add a library and index it **before resolving**.
   *
   * Blocking by design: the window shows a modal bar and stays unusable until
   * this settles, so nothing is ever half-indexed. `onProgress` is a required
   * channel — Tauri 2 cannot take an optional one.
   */
  addLibrary: (root: string, onProgress: (p: IndexProgress) => void, full = false) => {
    const channel = new Channel<IndexProgress>()
    channel.onmessage = onProgress
    return invoke<LibraryStatus>('add_library', { root, full, onProgress: channel })
  },

  listLibraries: () => invoke<LibraryStatus[]>('list_libraries'),

  removeLibrary: (root: string) => invoke<LibraryStatus[]>('remove_library', { root }),

  setActiveLibrary: (root: string) => invoke<LibraryStatus[]>('set_active_library', { root }),

  /**
   * Reindex one library, blocking.
   *
   * `full` re-probes every file and drops vanished ones; otherwise the pass only
   * looks at what changed. The library is named explicitly rather than defaulting
   * to the active one, because the sidebar's context menu acts on whichever
   * library was right-clicked.
   */
  reindexLibrary: (root: string, onProgress: (p: IndexProgress) => void, full = false) => {
    const channel = new Channel<IndexProgress>()
    channel.onmessage = onProgress
    return invoke<LibraryStatus>('reindex_library', { root, full, onProgress: channel })
  },

  search: (request: SearchRequest) => invoke<SoundRow[]>('search', { request }),

  /**
   * Every path the filter matches, for "select all".
   *
   * Separate from `search`: that one returns full rows and clamps at 5,000,
   * because a table page has no reason to be bigger. Selecting everything in a
   * filtered subset of a 280k library is not a page.
   */
  matchingPaths: (request: SearchRequest) => invoke<string[]>('list_matching_paths', { request }),

  /** Row count for the current filter, so "38 of 27,525" is never a guess. */
  countMatching: (request: SearchRequest) => invoke<number>('count_matching', { request }),

  folders: () => invoke<FolderNode>('list_folders'),

  /** Facet counts, favorites and total for the sidebar. */
  sidebar: () => invoke<Sidebar>('get_sidebar'),

  setFavorite: (paths: string[], favorite: boolean) =>
    invoke<number>('set_favorite', { paths, favorite }),

  setCategory: (paths: string[], category: string, subcategory?: string | null) =>
    invoke<number>('set_category', { paths, category, subcategory: subcategory ?? null }),

  clearCategory: (paths: string[]) => invoke<number>('clear_category', { paths }),

  undo: () => invoke<EditKind | null>('undo'),
  redo: () => invoke<EditKind | null>('redo'),
  history: () => invoke<HistoryState>('get_history'),

  /** Native folder picker. Resolves `null` when the user cancels. */
  pickLibraryFolder: () => invoke<string | null>('pick_library_folder'),

  revealInFolder: (path: string) => invoke<void>('reveal_in_folder', { path })
}

/**
 * Start an OS drag of `paths` out of the window so they can be dropped into a
 * DAW. This is the P0 feature, now wired to the list.
 *
 * The plugin ships no npm package in this project, so the command is invoked by
 * its raw name. Its Rust signature is
 * `start_drag(item, image, options, onEvent)`:
 *
 * - `image` is mandatory and must be a PNG data URL or a raw image object. Windows
 *   draws the drag cursor from the file list itself, so a transparent pixel is
 *   enough.
 * - `onEvent` is a `Channel` and is *also* mandatory — omitting it fails the whole
 *   command with `missing required key onEvent`, and the drag never starts. The
 *   channel reports `Dropped`/`Cancel`, which P0 measured as never firing; it is
 *   passed because the command requires it, not because anything listens.
 */
export async function startDrag(paths: string[]): Promise<void> {
  if (paths.length === 0) return
  await invoke('plugin:drag|start_drag', {
    item: paths,
    image: DRAG_IMAGE,
    options: { mode: 'copy' },
    onEvent: new Channel()
  })
}

/** 1x1 transparent PNG. The plugin rejects anything that is not a PNG data URL. */
const DRAG_IMAGE =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII='
