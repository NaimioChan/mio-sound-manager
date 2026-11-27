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
  phase: 'Scanning' | 'Comparing' | 'Indexing' | 'Finalizing' | 'Ready' | 'Failed'
  filesSeen: number
  filesChanged: number
  filesWritten: number
  metadataIndexing: boolean
  metadataIndexed: number
  /** Rows the file changed under; the next pass will pick them up. */
  metadataDeferred: number
  metadataTotal: number
  metadataFailed: number
  metadataStatus: 'idle' | 'indexing' | 'done' | 'failed' | 'cancelled'
  /** Whether the window is showing this library. */
  active: boolean
  skippedDirs: SkippedDir[]
}

export interface SkippedDir {
  path: string
  detail: string
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
  errorSeq: number
}

/**
 * The loop switch and its region.
 *
 * Mirrors `LoopState` in `crates/mio-app/src/state.rs`. The region is half-open —
 * `[start, end)` — and `null` means the whole clip, which is what the switch means
 * when nothing is selected on the waveform.
 */
export interface LoopState {
  enabled: boolean
  region: [number, number] | null
}

export interface Waveform {
  peaks: number[]
  /**
   * One envelope per channel, left first, for mono and stereo files. Empty for
   * files with more than two channels: those draw `peaks` in a single lane.
   */
  lanes: number[][]
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
  description: string | null
  descriptionCount: number | null
  ucsCategory: string | null
  ucsSubcategory: string | null
  ucsCatId: string | null
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

export interface UcsFacet {
  category: string
  subcategory: string | null
  count: number
}

export interface UcsCategory {
  catId: string
  category: string
  subcategory: string | null
  /**
   * The 22-category target, or `null` when this CatID is deliberately not
   * projected (archive marker, compressed air, a process word, or a CatID the
   * corpus contradicts). `null` is not `Uncategorized`: picking it would write a
   * manual Uncategorized and lock the row against the classifier, so the dialog
   * refuses to apply it.
   */
  projectedCategory: string | null
  projectedSubcategory: string | null
  count: number
}

export interface Sidebar {
  facets: Facet[]
  ucsFacets: UcsFacet[]
  favorites: number
  total: number
}

/** What an undoable edit was. The words for these live in `locales/`. */
export type EditKind = 'favorite' | 'unfavorite' | 'setCategory' | 'clearCategory' | 'setUcsCategory'

export interface HistoryState {
  undoKind: EditKind | null
  redoKind: EditKind | null
}

/** One tick from a blocking index pass. */
export interface IndexProgress {
  phase: 'Scanning' | 'Comparing' | 'Indexing' | 'Finalizing' | 'Ready' | 'Failed'
  done: number
  total: number
  filesSeen: number
  filesChanged: number
  filesWritten: number
}

export interface SearchRequest {
  query?: string
  maxDuration?: number
  channels?: number
  /** Several categories are OR-ed; empty means no category filter. */
  categories?: string[]
  /** Intersects with `categories`, which is what the rail's lit rows mean. */
  subcategories?: string[]
  ucsCategories?: string[]
  ucsSubcategories?: string[]
  ucsCatIds?: string[]
  folder?: string
  minConfidence?: number
  favoriteOnly?: boolean
  sortBy?: string
  sortDir?: 'asc' | 'desc'
  limit?: number
  offset?: number
}

export interface PlaybackError {
  path: string
  positionS: number
}

/** A bulk edit whose targets are resolved by the current filter in Rust. */
export interface MatchingEditRequest {
  request: SearchRequest
  exclude?: string[]
  /** Optional ordered window; omitted/zero limit means the complete match set. */
  offset?: number
  limit?: number
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
  takePlaybackError: () => invoke<PlaybackError | null>('take_playback_error'),

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

  prefetchNeighbors: (paths: string[]) => invoke<void>('prefetch_neighbors', { paths }),

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

  /** Row count for the current filter, so "38 of 27,525" is never a guess. */
  countMatching: (request: SearchRequest) => invoke<number>('count_matching', { request }),

  folders: () => invoke<FolderNode>('list_folders'),

  /** Facet counts, favorites and total for the sidebar. */
  sidebar: () => invoke<Sidebar>('get_sidebar'),

  ucsCategories: () => invoke<UcsCategory[]>('list_ucs_categories'),

  setFavorite: (paths: string[], favorite: boolean) =>
    invoke<number>('set_favorite', { paths, favorite }),

  setCategory: (paths: string[], category: string, subcategory?: string | null) =>
    invoke<number>('set_category', { paths, category, subcategory: subcategory ?? null }),

  setUcsCategory: (
    paths: string[],
    value: UcsCategory
  ) => invoke<number>('set_ucs_category', {
    paths,
    category: value.category,
    subcategory: value.subcategory,
    catId: value.catId,
    projectedCategory: value.projectedCategory,
    projectedSubcategory: value.projectedSubcategory
  }),

  setUcsCategoryMatching: (request: MatchingEditRequest, value: UcsCategory) =>
    invoke<number>('set_ucs_category_matching', {
      request,
      category: value.category,
      subcategory: value.subcategory,
      catId: value.catId,
      projectedCategory: value.projectedCategory,
      projectedSubcategory: value.projectedSubcategory
    }),

  clearCategory: (paths: string[]) => invoke<number>('clear_category', { paths }),

  setFavoriteMatching: (request: MatchingEditRequest, favorite: boolean) =>
    invoke<number>('set_favorite_matching', { request, favorite }),

  setCategoryMatching: (
    request: MatchingEditRequest,
    category: string,
    subcategory?: string | null
  ) => invoke<number>('set_category_matching', { request, category, subcategory: subcategory ?? null }),

  clearCategoryMatching: (request: MatchingEditRequest) =>
    invoke<number>('clear_category_matching', { request }),

  undo: () => invoke<EditKind | null>('undo'),
  redo: () => invoke<EditKind | null>('redo'),
  history: () => invoke<HistoryState>('get_history'),

  /**
   * Loop the loaded clip, optionally within `[startS, endS)`.
   *
   * A separate command from `play` on purpose: flipping the switch on a clip that
   * is already loaded must not re-publish it, or every flip would click. `null`
   * region means the whole clip.
   */
  setLoop: (enabled: boolean, region: [number, number] | null) =>
    invoke<LoopState>('set_loop', {
      enabled,
      startS: region ? region[0] : null,
      endS: region ? region[1] : null
    }),

  /** The loop switch and its region, as the engine holds them. */
  loop: () => invoke<LoopState>('get_loop'),

  /**
   * Render `[startS, endS)` of `path` to a WAV and return its path.
   *
   * The drag path needs a real file: a DAW accepts a drop of one, not a frame
   * range. Written into the app's cache, never into the library.
   */
  exportRegion: (path: string, startS: number, endS: number) =>
    invoke<string>('export_region', { path, startS, endS }),

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
