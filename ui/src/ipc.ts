// The IPC contract, in one place.
//
// Every type here mirrors a `#[derive(Serialize)]` struct in
// `crates/reslip-app/src/commands.rs`. They are hand-written rather than
// generated because the surface is small and a generator would need a build step
// that runs before the webview bundles — the one thing that must not fail on a
// machine without Rust installed.
//
// Field names are camelCase on the wire. Rust does the rename through
// `#[serde(rename_all = "camelCase")]`, so never send a snake_case payload.

import { Channel, invoke } from '@tauri-apps/api/core'

export interface LibraryStatus {
  root: string
  count: number
  indexing: boolean
  indexed: number
  total: number
}

export interface AppStatus {
  audioAvailable: boolean
  audioError: string | null
  deviceName: string | null
  sampleRate: number | null
  channels: number | null
  sampleFormat: string | null
  endpointFrames: number | null
  enginePeriodMs: number | null
  library: LibraryStatus | null
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
  channelLabel: string
  sampleRate: number | null
  filesize: number | null
  category: string
  subcategory: string | null
  confidence: number
  matchedTerms: string[]
  favorite: boolean
  categoryManual: boolean
  folder: string | null
}

export interface FolderNode {
  name: string
  path: string
  count: number
  children: FolderNode[]
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

export interface SearchRequest {
  query?: string
  maxDuration?: number
  channels?: number
  category?: string
  subcategory?: string
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

  waveform: (path: string, buckets?: number) =>
    invoke<Waveform>('get_waveform', { path, buckets: buckets ?? null }),

  openLibrary: (root: string) => invoke<LibraryStatus>('open_library', { root }),

  library: () => invoke<LibraryStatus | null>('get_library'),

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

