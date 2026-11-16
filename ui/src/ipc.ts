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

import { invoke } from '@tauri-apps/api/core'

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

  position: () => invoke<PlaybackPosition>('get_playback_position'),

  waveform: (path: string, buckets?: number) =>
    invoke<Waveform>('get_waveform', { path, buckets: buckets ?? null }),

  openLibrary: (root: string) => invoke<LibraryStatus>('open_library', { root }),

  library: () => invoke<LibraryStatus | null>('get_library'),

  search: (request: SearchRequest) => invoke<SoundRow[]>('search', { request }),

  folders: () => invoke<FolderNode>('list_folders')
}
