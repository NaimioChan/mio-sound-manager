import { Show, createEffect, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { getVersion } from '@tauri-apps/api/app'
import {
  api,
  startDrag,
  type AppStatus,
  type EditKind,
  type Facet,
  type FolderMenuRequest,
  type FolderNode,
  type IndexProgress,
  type LibraryStatus,
  type PlaybackError,
  type SoundRow,
  type UcsFacet,
  type UcsCategory
} from './ipc'
import {
  SoundTable,
  type EditScope,
  type FavoriteOverride,
  type RowMenuRequest
} from './SoundTable'
import {
  COLUMNS,
  DEFAULT_DIR,
  SORT_WIRE,
  hiddenColumns,
  toggleColumn,
  type SortKey
} from './columns'
import { WaveformDock, type Region } from './WaveformDock'
import { Sidebar, type SidebarMode } from './Sidebar'
import { Inspector } from './Inspector'
import { ContextMenu, type MenuItem } from './ContextMenu'
import { CategoryDialog } from './CategoryDialog'
import { SettingsDialog, loadSettings, saveSettings, type Settings } from './SettingsDialog'
import { IndexOverlay } from './IndexOverlay'
import { UNCATEGORIZED, categoryName, editKindName, t } from './i18n'
import { formatBpm, sampleRateLabel } from './format'
import { ICON } from './icons'
import { focusedElement, isTextEntry } from './keyboard'
// The product mark, next to the name in the top bar. A file rather than inline
// SVG: Vite hashes and emits it, so the CSP's `img-src 'self'` covers it and the
// logo stays one asset instead of a second copy of the paths.
import brandLogo from './assets/brand.svg'
import { applyTheme, loadThemeId } from './theme'
import { applyZoom, loadZoom } from './zoom'

/**
 * The message `AppError::Superseded` serialises to.
 *
 * A bare message is what the IPC boundary sends, so recognising "the user moved
 * on" is a string match. Keep it identical to the `#[error(...)]` attribute in
 * `crates/mio-app/src/error.rs`; if the variant ever grows a code field this can
 * go away.
 */
const SUPERSEDED_MESSAGE = 'superseded by a newer request'

/**
 * Playhead polling, per `docs/architecture.md` 7.3: read position from
 * `requestAnimationFrame`, never push events out of the audio callback.
 *
 * The loop issues a new IPC request only once the previous one resolves. At 60Hz
 * that would otherwise be 60 round trips a second queued on a machine that cannot
 * answer them, and every answer would arrive late and be drawn out of order.
 */
function usePlayhead() {
  const [positionS, setPositionS] = createSignal(0)
  const [durationS, setDurationS] = createSignal(0)
  const [playing, setPlaying] = createSignal(false)
  const [paused, setPaused] = createSignal(false)
  const [playbackError, setPlaybackError] = createSignal<PlaybackError | null>(null)
  let lastErrorSeq = 0

  let frame = 0
  let inFlight = false
  let stopped = false
  let lastPoll = 0

  /**
   * The idle/paused poll interval. Nothing changes while nothing plays, so 60
   * reads a second were 60 wasted IPC round trips; 4Hz is still fast enough
   * that pressing play shows the head moving within one interval.
   */
  const IDLE_INTERVAL_MS = 250

  const tick = async (ts: number) => {
    if (stopped) return
    // Playing is drawn every frame. Paused and idle only need to notice the
    // next state change, so they poll at 4Hz — and a hidden window paints
    // nothing, so it does not poll at all.
    const due = playing() || ts - lastPoll >= IDLE_INTERVAL_MS
    if (!inFlight && due && !document.hidden) {
      inFlight = true
      lastPoll = ts
      try {
        const p = await api.position()
        setPositionS(p.positionS)
        setDurationS(p.durationS)
        setPlaying(p.playing)
        setPaused(p.paused)
        if (p.errorSeq !== lastErrorSeq) {
          const error = await api.takePlaybackError()
          lastErrorSeq = p.errorSeq
          if (error) setPlaybackError(error)
        }
      } catch {
        // A dropped poll is not fatal; the next frame asks again.
      } finally {
        inFlight = false
      }
    }
    frame = requestAnimationFrame(tick)
  }

  onMount(() => {
    frame = requestAnimationFrame(tick)
  })

  onCleanup(() => {
    stopped = true
    cancelAnimationFrame(frame)
  })

  return { positionS, durationS, playing, paused, playbackError, clearPlaybackError: () => setPlaybackError(null) }
}

/** Sidebar width, remembered per machine. Clamped to what the layout can take. */
const SIDEBAR_KEY = 'mio:sidebar-width'
const SIDEBAR_MIN = 180
const SIDEBAR_MAX = 520
/** Output gain, remembered per machine. */
const GAIN_KEY = 'mio:gain'
/**
 * The level to return to when un-muting.
 *
 * Muting is not "set the volume to zero" — it is a temporary silence that must be
 * reversible to the level the user chose. Remembering it under its own key (rather
 * than inside `GAIN_KEY`) is what keeps a mute from destroying the stored level:
 * the slider's value and the mute state are two facts, and a single key can only
 * hold one.
 */
const UNMUTE_KEY = 'mio:unmute-gain'

/**
 * Which facet tree the rail shows, remembered per machine.
 *
 * The mode used to live in the rail while the filter-clearing lived here, so two
 * copies of one fact had to agree. It is now read here once and the rail takes it
 * as a prop. `mio:facet-mode` is the old two-value key: `mio` was Categories, and
 * anything else was UCS, which is still the default.
 */
const SIDEBAR_MODE_KEY = 'mio:sidebar-mode'
const SIDEBAR_MODES: SidebarMode[] = ['categories', 'ucs', 'folders']

/** The inspector's saved open state and width, outside the settings blob. */
const INSPECTOR_OPEN_KEY = 'mio:inspector-open'
const INSPECTOR_WIDTH_KEY = 'mio:inspector-width'
const INSPECTOR_MIN = 240
const INSPECTOR_MAX = 480
const INSPECTOR_DEFAULT = 300

function loadInspectorOpen(): boolean {
  return localStorage.getItem(INSPECTOR_OPEN_KEY) === '1'
}

function loadInspectorWidth(): number {
  const stored = Number(localStorage.getItem(INSPECTOR_WIDTH_KEY))
  if (!Number.isFinite(stored) || stored <= 0) return INSPECTOR_DEFAULT
  return Math.min(INSPECTOR_MAX, Math.max(INSPECTOR_MIN, Math.round(stored)))
}

function loadSidebarMode(): SidebarMode {
  const stored = localStorage.getItem(SIDEBAR_MODE_KEY)
  if (stored && (SIDEBAR_MODES as string[]).includes(stored)) return stored as SidebarMode
  return localStorage.getItem('mio:facet-mode') === 'mio' ? 'categories' : 'ucs'
}

const formatSampleRate = (rate: number | null): string => {
  if (rate === null) return '—'
  const decimals = rate % 1000 === 0 ? 0 : rate % 100 === 0 ? 1 : 2
  return `${(rate / 1000).toFixed(decimals)} kHz`
}

/**
 * A stored gain, or `fallback` when nothing usable is stored.
 *
 * `Number(x) || fallback` is wrong here and was a real bug: a stored `0` — the
 * muted level — is falsy, so a reload while muted came back at 100% and the next
 * un-mute wrote that over the remembered level. Only a missing or non-numeric
 * value may fall back.
 */
function readStoredGain(key: string, fallback: number): number {
  const raw = localStorage.getItem(key)
  if (raw === null) return fallback
  const value = Number(raw)
  if (!Number.isFinite(value)) return fallback
  return Math.min(1, Math.max(0, value))
}

export function App() {
  const [settings, setSettings] = createSignal<Settings>(loadSettings())
  /**
   * The applied theme.
   *
   * Separate from `settings` on purpose: the theme is written by `theme.ts` so the
   * script in `index.html` can read it before the bundle loads, and keeping a second
   * copy inside the settings blob is what made a reload fall back to the default —
   * two writers, one of them stale.
   */
  const [theme, setTheme] = createSignal(loadThemeId())
  /**
   * The interface scale, as a percentage.
   *
   * A signal as well as a webview property because the select mirrors it: the
   * store is the source of truth (`zoom.ts`), the webview is what actually scales,
   * and this keeps the two in step when a change fails to apply.
   */
  const [zoom, setZoom] = createSignal(loadZoom())

  const [status, setStatus] = createSignal<AppStatus | null>(null)
  const [bootError, setBootError] = createSignal<string | null>(null)
  const [libraries, setLibraries] = createSignal<LibraryStatus[]>([])
  const [metadataLibrary, setMetadataLibrary] = createSignal<LibraryStatus | null>(null)
  const [matching, setMatching] = createSignal(0)
  const [dataEpoch, setDataEpoch] = createSignal(0)
  const [folders, setFolders] = createSignal<FolderNode | null>(null)
  const [facets, setFacets] = createSignal<Facet[]>([])
  const [ucsFacets, setUcsFacets] = createSignal<UcsFacet[]>([])
  const [favorites, setFavorites] = createSignal(0)
  const [total, setTotal] = createSignal(0)

  const [searchDraft, setSearchDraft] = createSignal('')
  const [queryTerms, setQueryTerms] = createSignal<string[]>([])
  /**
   * The rail's lit rows, as sets rather than one pair.
   *
   * A sample hunt is usually "kicks and snares", not one kind at a time, and the
   * index filters with `IN (…)`. The two sets intersect — a category and one of
   * its subcategories lit together narrows to that subcategory — which is what
   * the rail visually claims.
   */
  const [categories, setCategories] = createSignal<string[]>([])
  /**
   * Subcategory rows, each keeping the category it belongs to.
   *
   * The wire only carries subcategory names, but the rail needs the parent: two
   * categories can share a subcategory name, and removing a category has to drop
   * the rows underneath it or the filter intersects with nothing and the list
   * goes empty for no visible reason.
   */
  const [subcategories, setSubcategories] = createSignal<{ category: string; sub: string }[]>([])
  const [ucsCategories, setUcsCategories] = createSignal<string[]>([])
  const [ucsSubcategories, setUcsSubcategories] = createSignal<{ category: string; sub: string }[]>([])
  const [ucsCatIds, setUcsCatIds] = createSignal<string[]>([])
  const [folder, setFolder] = createSignal<string | null>(null)
  const [favoritesOnly, setFavoritesOnly] = createSignal(false)
  const [sidebarMode, setSidebarMode] = createSignal<SidebarMode>(loadSidebarMode())

  /**
   * The channel-count, sample-rate, tempo and exact-cover filters.
   *
   * One value each, because clicking a different count means "that one instead":
   * equality filters intersect, so two counts would always match nothing.
   */
  const [channelsFilter, setChannelsFilter] = createSignal<number | null>(null)
  const [sampleRateFilter, setSampleRateFilter] = createSignal<number | null>(null)
  const [bpmFilter, setBpmFilter] = createSignal<number | null>(null)
  const [coverFilter, setCoverFilter] = createSignal<string | null>(null)

  /**
   * The inspector.
   *
   * The open state and width live here rather than inside the component because
   * the layout track has to reserve the width *around* the table: a panel that
   * sized itself would desynchronise the grid from the table's ResizeObserver on
   * the frame after every drag.
   */
  const [inspectorOpen, setInspectorOpen] = createSignal(loadInspectorOpen())
  const [inspectorW, setInspectorW] = createSignal(loadInspectorWidth())
  /**
   * Bumped when a details pass finishes, a library switches, or a full reindex
   * completes.
   *
   * Deliberately separate from `dataEpoch`: that one drops every cached segment
   * and re-queries the list. New thumbnails and tags change no row's position or
   * membership, so they must not cost a query — only the cover cells and the
   * inspector re-read what they had cached.
   */
  const [coverEpoch, setCoverEpoch] = createSignal(0)

  const [sortKey, setSortKey] = createSignal<SortKey>('filename')
  const [sortAsc, setSortAsc] = createSignal(true)

  /**
   * How many rows are selected, and nothing more.
   *
   * The paths themselves live in the table as one Set; keeping a copy here as
   * well was the Ctrl+A memory spike — three live copies of every path on a
   * 280k library. The app only ever read the length.
   */
  const [selectionCount, setSelectionCount] = createSignal(0)
  const [selectionReset, setSelectionReset] = createSignal(0)
  const clearSelection = () => {
    setSelectionCount(0)
    setSelectionReset((value) => value + 1)
  }
  const [active, setActive] = createSignal<SoundRow | null>(null)

  const [menu, setMenu] = createSignal<RowMenuRequest | null>(null)
  /**
   * A library header's right-click menu. Separate from `menu` because the two have
   * different targets and different actions: one acts on sounds, this one on a
   * library. Sharing the state would mean a union the renderer has to narrow on
   * every read, and the menus never open at the same time anyway.
   */
  const [libMenu, setLibMenu] = createSignal<{ library: LibraryStatus; x: number; y: number } | null>(
    null
  )
  /** A folder row's right-click menu. Third target, same hand-drawn panel. */
  const [folderMenu, setFolderMenu] = createSignal<FolderMenuRequest | null>(null)
  /** The header's right-click menu: show/hide columns, open column settings. */
  const [headerMenu, setHeaderMenu] = createSignal<{ x: number; y: number } | null>(null)
  const [categoryDialog, setCategoryDialog] = createSignal<{
    targets: SoundRow[]
    scope: EditScope
    count: number
  } | null>(null)
  const [ucsCatalog, setUcsCatalog] = createSignal<UcsCategory[]>([])
  const [settingsOpen, setSettingsOpen] = createSignal(false)
  /** Which section the settings dialog should scroll to, when it was opened from a menu. */
  const [settingsFocus, setSettingsFocus] = createSignal<'columns' | undefined>(undefined)
  const [history, setHistory] = createSignal<{ undo: EditKind | null; redo: EditKind | null }>({
    undo: null,
    redo: null
  })

  const [notice, setNotice] = createSignal<string | null>(null)
  const [metadataFailureShown, setMetadataFailureShown] = createSignal('')
  const [appVersion, setAppVersion] = createSignal('')

  /** A blocking index pass is running; the window is locked behind the overlay. */
  const [indexing, setIndexing] = createSignal<{ label: string; progress: IndexProgress | null } | null>(
    null
  )
  /**
   * A first-page query is in flight.
   *
   * Every filter or sort change re-queries and then swaps the row set. Nothing is
   * blocked while that happens — the old rows stay clickable — but the swap
   * detaches them, and a click that lands in the gap is applied to a row that is
   * about to disappear. Publishing the flag gives the UI (and `tools/` CDP
   * checks) a deterministic signal instead of a guessed sleep.
   */
  const [querying, setQuerying] = createSignal(false)

  const [sidebarW, setSidebarW] = createSignal(
    Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Number(localStorage.getItem(SIDEBAR_KEY)) || 232))
  )

  /**
   * The output level. Kept in the renderer as well as the engine so the slider
   * tracks the pointer without waiting for a round trip; `setGain` is fired on
   * every move and the engine clamps to what it actually applied.
   */
  const [gain, setGain] = createSignal(readStoredGain(GAIN_KEY, 1))
  /**
   * The level un-muting returns to.
   *
   * Starts at unity, and is only ever written while muted — so it always holds a
   * level the user actually chose, never the zero that a mute wrote. Stored, so a
   * reload while muted still un-mutes to the right level.
   */
  const [unmuteGain, setUnmuteGain] = createSignal(readStoredGain(UNMUTE_KEY, 1))

  /** The waveform selection, in seconds. Cleared whenever a different sound loads. */
  const [region, setRegion] = createSignal<Region | null>(null)
  /** Whether the engine loops. Mirrors `get_loop`, not a local guess. */
  const [looping, setLooping] = createSignal(false)
  /** True while a waveform selection is being rendered to a file for a drag. */
  const [exportingRegion, setExportingRegion] = createSignal(false)

  const playhead = usePlayhead()

  /**
   * Show a playback-command failure, unless it is the "a newer request arrived"
   * case.
   *
   * Superseding is normal: holding an arrow key or clicking the waveform faster
   * than a decode completes cancels the older request by design. Showing it put an
   * English backend string in the top bar on every other keypress, and it was not
   * translatable because it was born in Rust (`AGENTS.md`).
   */
  const showPlaybackError = (error: unknown) => {
    const message = String(error)
    if (message.includes(SUPERSEDED_MESSAGE)) return
    setNotice(message)
  }

  createEffect(() => {
    const error = playhead.playbackError()
    if (!error) return
    const seconds = Math.max(0, error.positionS)
    const minutes = Math.floor(seconds / 60)
    const rest = Math.floor(seconds % 60).toString().padStart(2, '0')
    setNotice(
      t('notice.playbackInterrupted', {
        name: error.path.split(/[\\/]/).pop() ?? error.path,
        time: `${minutes}:${rest}`
      })
    )
    playhead.clearPlaybackError()
  })

  const activeLibrary = createMemo(() => libraries().find((l) => l.active) ?? null)
  // Worker completion order is unrelated to click order. Keep writes, undo and
  // library switches in order without blocking the renderer.
  let libraryAction = Promise.resolve()
  const queueLibraryAction = (action: () => Promise<void>) => {
    libraryAction = libraryAction.then(action).catch((e) => {
      setNotice(String(e))
    })
    return libraryAction
  }
  let searchSequence = 0

  /**
   * The search request, derived once so the search call, "select all" and the
   * next page cannot drift apart.
   *
   * `sortBy` is the index's wire name, not the table's field name: see
   * `SORT_WIRE`. Sending `durationS` there is what made every search after a
   * Length click fail.
   */
  const request = createMemo(() => ({
    query: queryTerms().join(' '),
    categories: categories().length > 0 ? categories() : undefined,
    subcategories:
      subcategories().length > 0 ? subcategories().map((entry) => entry.sub) : undefined,
    ucsCategories: ucsCategories().length > 0 ? ucsCategories() : undefined,
    ucsSubcategories:
      ucsSubcategories().length > 0 ? ucsSubcategories().map((entry) => entry.sub) : undefined,
    ucsCatIds: ucsCatIds().length > 0 ? ucsCatIds() : undefined,
    folder: folder() ?? undefined,
    favoriteOnly: favoritesOnly() ? true : undefined,
    channels: channelsFilter() ?? undefined,
    sampleRate: sampleRateFilter() ?? undefined,
    bpm: bpmFilter() ?? undefined,
    coverHash: coverFilter() ?? undefined,
    sortBy: SORT_WIRE[sortKey()],
    sortDir: (sortAsc() ? 'asc' : 'desc') as 'asc' | 'desc'
  }))

  /** Recount results and invalidate cached windows for the new query snapshot. */
  const runSearch = async () => {
    const sequence = ++searchSequence
    if (libraries().length === 0) {
      setFavoriteOverride(null)
      setDataEpoch((epoch) => epoch + 1)
      setMatching(0)
      setQuerying(false)
      return
    }
    setQuerying(true)
    try {
      const count = await api.countMatching(request())
      if (sequence !== searchSequence) return
      // Windows for the new epoch read the committed flags from SQLite.
      setFavoriteOverride(null)
      setDataEpoch((epoch) => epoch + 1)
      setMatching(count)
    } catch (e) {
      if (sequence !== searchSequence) return
      setDataEpoch((epoch) => epoch + 1)
      setNotice(String(e))
    } finally {
      if (sequence === searchSequence) setQuerying(false)
    }
  }

  const fetchWindow = (offset: number, limit: number) =>
    api.search({ ...request(), offset, limit })

  const commitSearchTerm = () => {
    const term = searchDraft().trim()
    setSearchDraft('')
    if (!term || queryTerms().some((existing) => existing.toLocaleLowerCase() === term.toLocaleLowerCase())) return
    setQueryTerms([...queryTerms(), term])
    void runSearch()
  }

  const removeSearchTerm = (term: string) => {
    setQueryTerms(queryTerms().filter((existing) => existing !== term))
    void runSearch()
  }

  const clearSearchTerms = () => {
    if (queryTerms().length === 0) return
    setQueryTerms([])
    void runSearch()
  }

  const loadTree = async () => {
    try {
      setFolders(await api.folders())
    } catch {
      setFolders(null)
      // A remembered old library may still be running its deferred metadata
      // migration. Retry on the next status tick instead of leaving Folders
      // permanently empty after the first SQLite busy/read snapshot.
      if (activeLibrary()) {
        window.setTimeout(() => void loadTree(), 1000)
      }
    }
  }

  /**
   * Facet counts. Refreshed after every library edit rather than on a timer: a
   * stale count next to a category the user just emptied is the kind of wrong
   * number that makes the whole rail untrustworthy.
   *
   * The UCS CatID catalog is a separate call on purpose. It is the dialog's list of
   * every CatID with its count, and `set_favorite` cannot change it — but this used
   * to fetch it anyway, which put a whole-table `GROUP BY` behind every star click.
   * `ucsCatalog` says whether the caller's edit can actually move those numbers.
   */
  const loadSidebar = async (wantUcsCatalog = true) => {
    try {
      const [sidebar, catalog] = await Promise.all([
        api.sidebar(),
        wantUcsCatalog ? api.ucsCategories() : Promise.resolve(null)
      ])
      setFacets(sidebar.facets)
      setUcsFacets(sidebar.ucsFacets)
      setFavorites(sidebar.favorites)
      setTotal(sidebar.total)
      if (catalog) setUcsCatalog(catalog)
    } catch {
      setFacets([])
      setUcsFacets([])
    }
  }

  const loadHistory = async () => {
    try {
      const state = await api.history()
      setHistory({ undo: state.undoKind, redo: state.redoKind })
    } catch {
      setHistory({ undo: null, redo: null })
    }
  }

  /** Re-read everything about the active library and the open-library list. */
  const refresh = async () => {
    try {
      const next = await api.status()
      setStatus(next)
      setBootError(null)
      const nextLibraries = await api.listLibraries()
      setLibraries(nextLibraries)
      const active = nextLibraries.find((library) => library.active) ?? null
      setMetadataLibrary(active?.metadataIndexing ? active : null)
      if (active?.metadataStatus === 'failed' && active.metadataFailed > 0) {
        const key = `${active.root}:${active.metadataFailed}`
        if (metadataFailureShown() !== key) {
          setMetadataFailureShown(key)
          setNotice(t('notice.metadataFailed', { count: active.metadataFailed }))
        }
      }
      if (next.library) {
        await Promise.all([loadTree(), loadSidebar(), loadHistory()])
      }
      await runSearch()
    } catch (e) {
      setBootError(String(e))
    }
  }

  const play = async (row: SoundRow, startAt?: number) => {
    setActive(row)
    // A selection belongs to the sound it was drawn on. Keeping it across a
    // different sound would either export the wrong region or loop a part of a
    // file the user never looked at.
    setRegion(null)
    playhead.clearPlaybackError()
    try {
      await api.play(row.path, startAt)
    } catch (e) {
      showPlaybackError(e)
    }
  }

  const stop = async () => {
    try {
      await api.stop()
    } catch (e) {
      setNotice(String(e))
    }
  }

  /** Space and the dock's Pause/Resume button both land here. */
  const togglePlayback = async () => {
    try {
      if (playhead.paused()) await api.resume()
      else if (playhead.playing()) await api.pause()
      else if (active()) await api.play(active()!.path)
    } catch (e) {
      showPlaybackError(e)
    }
  }

  /**
   * Move the output level.
   *
   * The signal is set first so the slider and the percentage follow the pointer
   * with no latency; the engine call is fire-and-forget because a dropped one
   * costs a level change the user will make again, not a broken state. The value
   * is persisted so the next launch starts where this one ended.
   *
   * Moving the slider off zero also records the level un-mute returns to. Doing it
   * here rather than in `toggleMute` means the remembered level is always one the
   * user dialled in, even if they muted, moved the slider and un-muted in some
   * other order.
   */
  const changeGain = (next: number) => {
    const clamped = Math.min(1, Math.max(0, next))
    setGain(clamped)
    localStorage.setItem(GAIN_KEY, String(clamped))
    if (clamped > 0) {
      setUnmuteGain(clamped)
      localStorage.setItem(UNMUTE_KEY, String(clamped))
    }
    api.setGain(clamped).catch(() => {
      // The engine clamps to its own range; a rejection here means no device,
      // and the level is remembered for whenever one opens.
    })
  }

  /**
   * Mute, or restore the level the user was listening at.
   *
   * A mute is a temporary silence, not a volume change: un-muting has to return to
   * the chosen level, and the engine only holds one number. So the level to come
   * back to lives in its own signal, written whenever the slider is moved off zero.
   */
  const toggleMute = () => {
    if (gain() > 0) {
      // Only remember a real level. Muting an already-muted output would store the
      // zero and un-mute to silence.
      setUnmuteGain(gain())
      localStorage.setItem(UNMUTE_KEY, String(gain()))
      changeGain(0)
      return
    }
    const restored = unmuteGain() > 0 ? unmuteGain() : 1
    changeGain(restored)
  }

  /**
   * Loop the loaded clip.
   *
   * The engine owns the state and this mirrors what it returns, so a rejected set
   * (no device) shows as the switch staying where it was rather than lying about
   * it. The region is the waveform's selection; with nothing selected the whole
   * clip loops.
   */
  const toggleLoop = async () => {
    const next = !looping()
    try {
      const state = await api.setLoop(next, next ? region() : null)
      setLooping(state.enabled)
    } catch (e) {
      setNotice(String(e))
    }
  }

  /**
   * Commit a selection, and re-arm the loop if it is on.
   *
   * A running loop has to follow the region it is looping: changing the selection
   * without telling the engine would keep it playing the old part, which reads as
   * the selection being ignored.
   */
  const changeRegion = (next: Region | null) => {
    setRegion(next)
    if (looping()) void api.setLoop(true, next).catch(() => setLooping(false))
  }

  /**
   * Drag-out from the waveform's top band: hand material to the OS as a file drop.
   *
   * Which material depends on where the gesture started, and that is the whole
   * rule: a press inside the selection carries the selection, a press anywhere else
   * carries the file. Nothing is exported speculatively — a drag that starts
   * outside the selection is the source file, untouched.
   *
   * A DAW takes a file, not a frame range, so a selection is rendered to a WAV in
   * the app's cache first and that path is what the drag carries.
   */
  const dragRegionOut = async (atSeconds: number) => {
    const row = active()
    if (!row) return
    const selected = region()
    const inside = selected !== null && atSeconds >= selected[0] && atSeconds <= selected[1]
    if (!selected || !inside) {
      await startDrag([row.path])
      return
    }
    setExportingRegion(true)
    try {
      const exported = await api.exportRegion(row.path, selected[0], selected[1])
      await startDrag([exported])
      setNotice(t('notice.regionExported', { name: exported.split(/[\\/]/).pop() ?? '' }))
    } catch (e) {
      setNotice(String(e))
    } finally {
      setExportingRegion(false)
    }
  }

  /**
   * Add a library. Blocks on the index pass and shows the overlay while it runs.
   *
   * `add_library` only resolves when the pass is done, so the overlay is driven by
   * the promise lifetime, not by a separate poll: there is no window in which the
   * app thinks it is idle while the database is still being written.
   */
  const addLibrary = (root?: string) => queueLibraryAction(() => openLibrary(root))

  const openLibrary = async (root?: string) => {
    let target = root
    if (!target) {
      const picked = await api.pickLibraryFolder()
      if (!picked) return
      target = picked
    }

    setIndexing({ label: t('index.indexing'), progress: null })
    try {
      const result = await api.addLibrary(target, (progress) => {
        setIndexing({ label: t('index.indexing'), progress })
      })
      setIndexing(null)
      if (result.skippedDirs.length > 0) {
        setNotice(
          t('notice.indexSkippedDirs', {
            count: result.skippedDirs.length,
            paths: result.skippedDirs.map((dir) => dir.path).join(', ')
          })
        )
      }
      clearSelection()
      setSettingsOpen(false)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setIndexing(null)
    }
  }

  /**
   * Reindex one library, blocking.
   *
   * Named explicitly rather than defaulting to the active one: the rail's menu
   * acts on whichever library was right-clicked, which may be in the background.
   */
  const reindexLibrary = async (root: string, full: boolean) => {
    setIndexing({ label: full ? t('index.fullReindex') : t('index.reindexing'), progress: null })
    try {
      const result = await api.reindexLibrary(root, (progress) => {
        setIndexing({ label: full ? t('index.fullReindex') : t('index.reindexing'), progress })
      }, full)
      setIndexing(null)
      if (result.skippedDirs.length > 0) {
        setNotice(
          t('notice.indexSkippedDirs', {
            count: result.skippedDirs.length,
            paths: result.skippedDirs.map((dir) => dir.path).join(', ')
          })
        )
      }
      clearSelection()
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setIndexing(null)
    }
  }

  const switchLibrary = async (root: string) => {
    await queueLibraryAction(() => activateLibrary(root))
  }

  const activateLibrary = async (root: string) => {
    if (activeLibrary()?.root === root) return
    try {
      setLibraries(await api.setActiveLibrary(root))
      // A different database means every derived thing is stale: the facets, the
      // folder tree, the undo stack and the rows.
      setCategories([])
      setSubcategories([])
      setUcsCategories([])
      setUcsSubcategories([])
      setUcsCatIds([])
      setFolder(null)
      setFavoritesOnly(false)
      clearSelection()
      setActive(null)
      // A different database means every cached thumbnail and detail belongs to
      // a hash or a path this library may not even contain.
      setCoverEpoch((epoch) => epoch + 1)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const removeLibrary = async (root: string) => {
    await queueLibraryAction(() => closeLibrary(root))
  }

  const closeLibrary = async (root: string) => {
    try {
      setLibraries(await api.removeLibrary(root))
      clearSelection()
      await refresh()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const changeSettings = (patch: Partial<Settings>) => {
    const next = { ...settings(), ...patch }
    setSettings(next)
    saveSettings(next)
  }

  const indexingInBackground = createMemo(() => libraries().some((library) => library.indexing || library.metadataIndexing))
  createEffect(() => {
    if (!indexingInBackground()) return
    let cancelled = false
    let timer: number | undefined
    const poll = async () => {
      try {
        const previous = libraries().find((library) => library.active)
        const next = await api.listLibraries()
        if (cancelled) return
        setLibraries(next)
        const active = next.find((library) => library.active) ?? null
        setMetadataLibrary(active?.metadataIndexing ? active : null)
        if (active?.metadataStatus === 'failed' && active.metadataFailed > 0) {
          const key = `${active.root}:${active.metadataFailed}`
          if (metadataFailureShown() !== key) {
            setMetadataFailureShown(key)
            setNotice(t('notice.metadataFailed', { count: active.metadataFailed }))
          }
        }
        if (active && previous?.root === active.root &&
            ((previous.indexing && !active.indexing) || (previous.metadataIndexing && !active.metadataIndexing))) {
          // A finished pass means new tags and thumbnails exist. The rows did not
          // move, so only the cover cells and the inspector re-read; a `dataEpoch`
          // bump here would throw away every cached segment for new pictures.
          setCoverEpoch((epoch) => epoch + 1)
          void Promise.all([loadTree(), loadSidebar(), runSearch()])
        }
        if (next.some((library) => library.indexing || library.metadataIndexing)) {
          timer = window.setTimeout(poll, 350)
        }
      } catch {
        if (!cancelled) timer = window.setTimeout(poll, 1000)
      }
    }
    timer = window.setTimeout(poll, 350)
    onCleanup(() => {
      cancelled = true
      window.clearTimeout(timer)
    })
  })

  const changeTheme = (id: string) => {
    setTheme(applyTheme(id).id)
  }

  /**
   * Apply an interface scale.
   *
   * The select has already moved when this runs, so the only thing to do is make
   * the webview agree — and if it does not (no webview yet, a denied permission),
   * `applyZoom` resolves to the scale that is actually in effect and the select
   * snaps back instead of claiming a value the window does not have.
   */
  const changeZoom = (percent: number) => {
    void applyZoom(percent).then(setZoom)
  }

  const toggleAutoPlay = () => changeSettings({ autoPlay: !settings().autoPlay })

  const changeSort = (key: SortKey) => {
    if (sortKey() === key) {
      setSortAsc(!sortAsc())
    } else {
      setSortKey(key)
      setSortAsc(DEFAULT_DIR[key] === 'asc')
    }
    void runSearch()
  }

  /**
   * Hiding the column the list is sorted by would leave it sorted by an invisible
   * key, with no arrow to explain the order and no way to change it back except
   * through the header menu. Fall back to the name, which is always visible.
   */
  createEffect(() => {
    const hidden = hiddenColumns()
    if (!hidden.has(sortKey())) return
    setSortKey('filename')
    setSortAsc(true)
    void runSearch()
  })

  const changeSelection = (count: number, nextActive: SoundRow | null) => {
    setSelectionCount(count)
    if (nextActive) setActive(nextActive)
  }

  const onSelectAll = (count: number) => {
    setNotice(t('notice.selected', { count: count.toLocaleString() }))
  }

  /**
   * Clicking a row: select it, and audition it when auto-play is on.
   *
   * The keyboard's arrow keys go through the same path, so "clicking a row plays
   * it" and "walking the list plays each one" cannot diverge.
   */
  const activate = (row: SoundRow) => {
    setActive(row)
    if (settings().autoPlay) void play(row)
  }

  /**
   * Clicking a category in the rail.
   *
   * Plain click replaces the selection (and clicking the only lit category clears
   * it, so the row toggles like a filter should); Ctrl+click adds or removes it,
   * which is what makes "kicks and snares" one query instead of two.
   */
  const toggleCategory = (name: string | null, additive: boolean) => {
    if (name === null) {
      // The "All sounds" row: one click clears every category row.
      setCategories([])
      setSubcategories([])
      setUcsCategories([])
      setUcsSubcategories([])
      setUcsCatIds([])
      setFolder(null)
      setFavoritesOnly(false)
      void runSearch()
      return
    }
    if (!additive) {
      const sole = categories().length === 1 && categories()[0] === name
      setCategories(sole ? [] : [name])
      // A category click means "this whole category", so the subcategory rows
      // underneath it would otherwise narrow it to something the click did not ask
      // for.
      setSubcategories([])
    } else if (categories().includes(name)) {
      setCategories(categories().filter((c) => c !== name))
      setSubcategories(subcategories().filter((entry) => entry.category !== name))
    } else {
      setCategories([...categories(), name])
    }
    setFolder(null)
    setFavoritesOnly(false)
    void runSearch()
  }

  /**
   * Clicking a UCS top category.
   *
   * The CatIDs are the third filter level, and they intersect with the category
   * set: leaving a stale one behind (the old bug) produced
   * `ucs_category IN ('X') AND ucs_cat_id IN ('an id of Y')`, so both rows looked
   * lit and the list went empty. Every branch therefore maintains `ucsCatIds`
   * together with the category set:
   *
   * * not additive → the clicked category is the only one, so no catId narrows it;
   * * additive and removing → drop the catIds that belong to the removed
   *   category, since their parent is no longer selected;
   * * additive and adding → leave existing catIds alone; they still have their
   *   own parents selected.
   */
  const toggleUcsCategory = (name: string | null, additive: boolean) => {
    if (name === null) {
      setUcsCategories([])
      setUcsSubcategories([])
      setUcsCatIds([])
      setFolder(null)
      setFavoritesOnly(false)
      void runSearch()
      return
    }
    if (!additive) {
      const sole = ucsCategories().length === 1 && ucsCategories()[0] === name
      setUcsCategories(sole ? [] : [name])
      setUcsSubcategories([])
      setUcsCatIds([])
    } else if (ucsCategories().includes(name)) {
      setUcsCategories(ucsCategories().filter((category) => category !== name))
      setUcsSubcategories(ucsSubcategories().filter((entry) => entry.category !== name))
      const own = new Set(
        ucsCatalog()
          .filter((entry) => entry.category === name)
          .map((entry) => entry.catId)
      )
      setUcsCatIds(ucsCatIds().filter((catId) => !own.has(catId)))
    } else {
      setUcsCategories([...ucsCategories(), name])
    }
    setFolder(null)
    setFavoritesOnly(false)
    void runSearch()
  }

  /**
   * Clicking a subcategory. Its parent joins the category set, because the two
   * filters intersect: a lit subcategory with its parent missing would filter to
   * an empty list while both rows looked lit.
   */
  const toggleSubcategory = (category: string, sub: string, additive: boolean) => {
    const same = (entry: { category: string; sub: string }) =>
      entry.category === category && entry.sub === sub
    const lit = subcategories().some(same)

    let next: { category: string; sub: string }[]
    if (additive) {
      next = lit
        ? subcategories().filter((entry) => !same(entry))
        : [...subcategories(), { category, sub }]
    } else if (lit && subcategories().length === 1) {
      // Clicking the only lit subcategory turns the filter off, the same way a
      // second click on the only lit category does.
      next = []
    } else {
      next = [{ category, sub }]
    }

    setSubcategories(next)
    // Every parent of a lit subcategory must also be lit: the two filters
    // intersect, so an unlit parent would make the query match nothing.
    const parents = new Set(next.map((entry) => entry.category))
    if (!additive) {
      setCategories([...parents])
    } else {
      const kept = categories().filter((c) => c !== category || parents.has(c))
      setCategories([...new Set([...kept, ...parents])])
    }
    setFolder(null)
    setFavoritesOnly(false)
    void runSearch()
  }

  const toggleUcsSubcategory = (category: string, sub: string, additive: boolean, catId?: string) => {
    const same = (entry: { category: string; sub: string }) =>
      entry.category === category && entry.sub === sub
    const lit = ucsSubcategories().some(same)
    let next: { category: string; sub: string }[]
    if (additive) {
      next = lit
        ? ucsSubcategories().filter((entry) => !same(entry))
        : [...ucsSubcategories(), { category, sub }]
    } else if (lit && ucsSubcategories().length === 1) {
      next = []
    } else {
      next = [{ category, sub }]
    }
    setUcsSubcategories(next)
    const parents = new Set(next.map((entry) => entry.category))
    setUcsCategories(additive
      ? [...new Set([...ucsCategories().filter((value) => value !== category || parents.has(value)), ...parents])]
      : [...parents])
    setUcsCatIds(catId ? (additive ? [...new Set([...ucsCatIds(), catId])] : [catId]) : [])
    setFolder(null)
    setFavoritesOnly(false)
    void runSearch()
  }

  const changeSidebarMode = (next: SidebarMode) => {
    setSidebarMode(next)
    localStorage.setItem(SIDEBAR_MODE_KEY, next)
    // Switching trees clears the filters that belonged to the old tree. Folder and
    // favourites are their own chips, independent of the tree, and stay lit.
    setCategories([])
    setSubcategories([])
    setUcsCategories([])
    setUcsSubcategories([])
    setUcsCatIds([])
    void runSearch()
  }

  const setFolderFilter = (path: string | null) => {
    setFolder(path)
    setCategories([])
    setSubcategories([])
    setUcsCategories([])
    setUcsSubcategories([])
    setFavoritesOnly(false)
    void runSearch()
  }

  /**
   * Clear every search filter, without re-querying.
   *
   * Deliberately not built from the rail's toggles: `toggleCategory` and
   * `setFolderFilter` each clear a *different* set of neighbours (that is what a
   * click on one rail row means), so composing them loses filters the click did
   * not mention and runs a query per step. The inspector's exclusive clicks need
   * one state reset followed by exactly one query, and this is that reset.
   *
   * The active row is kept: the sound whose value was clicked is, by definition,
   * in the new result set, and dropping it would close the inspector (or blank
   * it) for the row the user is still looking at.
   */
  const clearSearchState = () => {
    setQueryTerms([])
    setSearchDraft('')
    setCategories([])
    setSubcategories([])
    setUcsCategories([])
    setUcsSubcategories([])
    setUcsCatIds([])
    setFolder(null)
    setFavoritesOnly(false)
    setChannelsFilter(null)
    setSampleRateFilter(null)
    setBpmFilter(null)
    setCoverFilter(null)
    clearSelection()
  }

  /**
   * Append one term to the query, keeping everything else.
   *
   * A value with no letter or digit is skipped: `fst::` or `—` would become an
   * empty FTS expression, and the list would silently clear. The dedupe is
   * case-insensitive because FTS5 treats `Door` and `door` as the same token.
   */
  const addQueryTerm = (text: string) => {
    const term = text.trim()
    if (!term || !/[0-9a-z]/i.test(term)) return
    const lower = term.toLocaleLowerCase()
    if (queryTerms().some((existing) => existing.toLocaleLowerCase() === lower)) return
    setQueryTerms([...queryTerms(), term])
    void runSearch()
  }

  /** A filter that replaces the whole search: clear, set one thing, one query. */
  const showCategoryExclusive = (category: string) => {
    clearSearchState()
    setCategories([category])
    void runSearch()
  }

  const showSubcategoryExclusive = (category: string, sub: string) => {
    clearSearchState()
    setCategories([category])
    setSubcategories([{ category, sub }])
    void runSearch()
  }

  const showUcsCategoryExclusive = (category: string) => {
    clearSearchState()
    setUcsCategories([category])
    void runSearch()
  }

  const showUcsSubcategoryExclusive = (category: string, sub: string) => {
    clearSearchState()
    setUcsCategories([category])
    setUcsSubcategories([{ category, sub }])
    void runSearch()
  }

  const showUcsCatIdExclusive = (catId: string) => {
    clearSearchState()
    setUcsCatIds([catId])
    void runSearch()
  }

  /**
   * Filter to one folder.
   *
   * The caller passes an absolute path. `SoundRow.folder` is relative to the
   * library root, and the backend's filter is a path range over absolute stored
   * paths; handing it the relative value would match nothing.
   */
  const showFolderExclusive = (absolute: string | null) => {
    clearSearchState()
    setFolder(absolute)
    void runSearch()
  }

  /**
   * A numeric filter: replace this field, or clear it when it already is the
   * value, and leave every other filter alone.
   */
  const toggleChannelsFilter = (channels: number) => {
    setChannelsFilter(channelsFilter() === channels ? null : channels)
    void runSearch()
  }

  const toggleSampleRateFilter = (rate: number) => {
    setSampleRateFilter(sampleRateFilter() === rate ? null : rate)
    void runSearch()
  }

  const toggleBpmFilter = (bpm: number) => {
    setBpmFilter(bpmFilter() === bpm ? null : bpm)
    void runSearch()
  }

  /**
   * Filter to one cover, by its bytes.
   *
   * "Same cover" is exact, not visual: two files carrying the same JPEG share a
   * hash, a re-encoded or re-scaled copy does not. That was accepted deliberately
   * — a perceptual hash would merge artwork the user did not ask to merge.
   */
  const showCoverExclusive = (key: string) => {
    clearSearchState()
    setCoverFilter(key)
    void runSearch()
  }

  const copyValue = (value: string) => {
    void navigator.clipboard.writeText(value)
    setNotice(t('notice.valueCopied', { value }))
  }

  /**
   * The panel width while dragging, and once the drag settles.
   *
   * The signal drives every pointermove (the table resizes through CSS grid with
   * no query), but writing `localStorage` per move would sync-flush the profile
   * dozens of times a second. A trailing write is enough: the value only has to
   * survive a reload.
   */
  let inspectorSave: number | undefined
  const changeInspectorWidth = (px: number) => {
    const clamped = Math.min(INSPECTOR_MAX, Math.max(INSPECTOR_MIN, Math.round(px)))
    setInspectorW(clamped)
    if (inspectorSave !== undefined) window.clearTimeout(inspectorSave)
    inspectorSave = window.setTimeout(() => {
      localStorage.setItem(INSPECTOR_WIDTH_KEY, String(inspectorW()))
    }, 200)
  }

  /**
   * A star click that did not need a re-query, applied at read time by the table.
   *
   * Kept for rows that are on screen: a star is a boolean per row, so the click
   * can paint the change itself. `flags` only holds the paths this click touched,
   * and `SoundTable.rowAt` merges them, so exactly those rows get a new object.
   */
  const [favoriteOverride, setFavoriteOverride] = createSignal<FavoriteOverride | null>(null)

  /**
   * Paint `paths` as `favorite` without a query, for the rows already loaded.
   *
   * Matching edits apply to a filter, not to the rows the table has loaded, so the
   * caller passes the paths it knows about; rows outside the cache stay as the
   * database has them, which is what a later fetch will show anyway.
   */
  const paintFavorites = (paths: string[], favorite: boolean) => {
    if (paths.length === 0) return
    const flags = new Map(favoriteOverride()?.flags ?? [])
    for (const path of paths) flags.set(path, favorite)
    setFavoriteOverride({ flags })
  }

  /**
   * Apply a library edit, then refresh exactly what that kind of edit can change.
   *
   * Starring used to refresh everything: `get_sidebar` (facets + summary + the UCS
   * CatID group-by) and `runSearch()` (which bumps `dataEpoch`, dropping every
   * cached segment). Nothing about a star changes a category count or a CatID, and
   * with no favourite filter and no favourite sort it changes no row's position —
   * the CatID group-by alone took 447 ms in a SQL probe on the 289k library.
   *
   * `paths` is what the click painted locally, so a local star also updates the
   * rows on screen. `null` means "do not paint", for callers that only know a
   * scope (the matching form) and whose rows are not loaded.
   */
  type EditRefreshOptions = { paths?: string[] | null; favorite?: boolean; resetSelection?: boolean }

  const editThenRefresh = (
    apply: () => Promise<number>,
    kind: EditKind,
    options: EditRefreshOptions = {}
  ) => {
    const root = activeLibrary()?.root
    return queueLibraryAction(async () => {
      if (!root || activeLibrary()?.root !== root) return
      await applyLibraryEdit(apply, kind, options)
    })
  }

  const applyLibraryEdit = async (
    apply: () => Promise<number>,
    kind: EditKind,
    options: EditRefreshOptions
  ) => {
    const { paths = null, favorite = false, resetSelection = false } = options
    try {
      const changed = await apply()
      if (resetSelection) clearSelection()
      // Nothing moved in the database: the history entry was not recorded either.
      if (changed === 0) return
      const favoriteKind = kind === 'favorite' || kind === 'unfavorite'
      // A star changes which rows match or their order only when the view filters
      // on the flag or sorts by it; otherwise the list on screen is still correct.
      // Matching edits have no exact path list and must invalidate cached windows.
      const starIsLocal =
        favoriteKind &&
        paths !== null &&
        paths.length > 0 &&
        !favoritesOnly() &&
        sortKey() !== 'favorite'
      if (starIsLocal) {
        paintFavorites(paths, favorite)
        await Promise.all([refreshFavoriteCount(), loadHistory()])
        return
      }
      setFavoriteOverride(null)
      // A star still moves the favourite *count*, but never a facet: it changes no
      // row's category. So the re-query path for a star pays `get_summary` and the
      // rows, not the two whole-table facet groups.
      if (favoriteKind) {
        await Promise.all([refreshFavoriteCount(), runSearch(), loadHistory()])
        return
      }
      // Facets move for a category edit. The UCS CatID catalog is rebuilt only when
      // the edit wrote a UCS label, because that is the one thing `ucs_cat_counts`
      // reads.
      await Promise.all([loadSidebar(kind === 'setUcsCategory'), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  }

  /**
   * The sidebar's favourite number and total, without the two facet `GROUP BY`s.
   *
   * A star moves `favorites` and nothing else; re-reading the facets bought two
   * whole-table groups the click cannot have changed.
   */
  const refreshFavoriteCount = async () => {
    try {
      const counts = await api.summary()
      setFavorites(counts.favorites)
      setTotal(counts.total)
    } catch {
      // A failed count leaves the old number; the star itself already moved.
    }
  }

  const undo = () => queueLibraryAction(async () => {
    try {
      const kind = await api.undo()
      if (!kind) return
      setNotice(t('notice.undid', { label: editKindName(kind) }))
      setFavoriteOverride(null)
      // Undo writes back whatever the edit changed, including UCS columns for a
      // category edit, so the catalog is rebuilt rather than assumed unchanged.
      await Promise.all([loadSidebar(true), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  })

  const redo = () => queueLibraryAction(async () => {
    try {
      const kind = await api.redo()
      if (!kind) return
      setNotice(t('notice.redid', { label: editKindName(kind) }))
      setFavoriteOverride(null)
      await Promise.all([loadSidebar(true), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  })

  const toggleFavorite = (row: SoundRow) => {
    const root = activeLibrary()?.root
    void queueLibraryAction(async () => {
      if (!root || activeLibrary()?.root !== root) return
      const next = !(favoriteOverride()?.flags.get(row.path) ?? row.favorite)
      await applyLibraryEdit(() => api.setFavorite([row.path], next), next ? 'favorite' : 'unfavorite', {
        paths: [row.path],
        favorite: next
      })
    })
  }

  const applyFavoriteScope = (scope: EditScope, favorite: boolean) => {
    if (scope.kind === 'paths') return api.setFavorite(scope.paths, favorite)
    return api.setFavoriteMatching(
      {
        request: scope.request,
        exclude: scope.exclude,
        offset: scope.offset,
        limit: scope.limit
      },
      favorite
    )
  }

  const applyCategoryScope = (scope: EditScope, category: string, sub: string | null) => {
    if (scope.kind === 'paths') return api.setCategory(scope.paths, category, sub)
    return api.setCategoryMatching(
      {
        request: scope.request,
        exclude: scope.exclude,
        offset: scope.offset,
        limit: scope.limit
      },
      category,
      sub
    )
  }

  const clearCategoryScope = (scope: EditScope) => {
    if (scope.kind === 'paths') return api.clearCategory(scope.paths)
    return api.clearCategoryMatching({
      request: scope.request,
      exclude: scope.exclude,
      offset: scope.offset,
      limit: scope.limit
    })
  }

  /**
   * The star entry in the row menu.
   *
   * `ContextMenu` closes the menu *before* it runs the item (so a click on a row
   * behind it does not also select that row), which sets `menu()` to `null`. These
   * handlers used to read `menu()` themselves and therefore did nothing at all —
   * a right-click "Add to Favorites" was silently a no-op. The request is captured
   * here, at build time, the same way "Set Category" captures it for the dialog.
   */
  const toggleFavoriteSelection = (request: RowMenuRequest) => {
    const targets = request.targets
    if (targets.length === 0) return
    const next = !targets.every((r) => r.favorite)
    void editThenRefresh(() => applyFavoriteScope(request.scope, next), next ? 'favorite' : 'unfavorite', {
      paths: request.scope.kind === 'paths' ? request.scope.paths : null,
      favorite: next,
      resetSelection: request.scope.kind === 'matching'
    })
  }

  const favoriteScope = (request: RowMenuRequest, favorite: boolean) => {
    void editThenRefresh(
      () => applyFavoriteScope(request.scope, favorite),
      favorite ? 'favorite' : 'unfavorite',
      {
        paths: request.scope.kind === 'paths' ? request.scope.paths : null,
        favorite,
        resetSelection: request.scope.kind === 'matching'
      }
    )
  }

  const reveal = (row: SoundRow) => {
    api.revealInFolder(row.path).catch((e) => setNotice(String(e)))
  }

  const copyPaths = (targets: SoundRow[]) => {
    void navigator.clipboard.writeText(targets.map((r) => r.path).join('\n'))
    setNotice(
      targets.length === 1
        ? t('notice.pathCopied')
        : t('notice.pathsCopied', { count: targets.length })
    )
  }

  const menuItems = (): MenuItem[] => {
    const current = menu()
    if (!current) return []
    const targets = current.targets
    const one = current.count === 1 && targets.length === 1 ? targets[0] : null
    const many = (label: string) =>
      current.count === 1 ? label : t('menu.withCount', { label, count: current.count })
    const allStarred = targets.length > 0 && targets.every((r) => r.favorite)

    return [
      {
        id: 'play',
        label: t('menu.play'),
        shortcut: 'Space',
        disabled: !one,
        onSelect: () => one && void play(one)
      },
      { id: 'd1', label: '', divider: true },
      ...(current.scope.kind === 'matching' || current.count > targets.length
        ? [
            {
              id: 'favorite',
              label: many(t('edit.favorite')),
              onSelect: () => favoriteScope(current, true)
            },
            {
              id: 'unfavorite',
              label: many(t('edit.unfavorite')),
              onSelect: () => favoriteScope(current, false)
            }
          ]
        : [
            {
              id: 'favorite',
              label: many(allStarred ? t('edit.unfavorite') : t('edit.favorite')),
              onSelect: () => toggleFavoriteSelection(current)
            }
          ]),
      {
        id: 'category',
        label: many(t('menu.setCategory')),
        onSelect: () => setCategoryDialog({ targets, scope: current.scope, count: current.count })
      },
      {
        id: 'clear-category',
        label: many(t('menu.clearCategory')),
        disabled:
          current.scope.kind === 'paths' &&
          current.count === targets.length &&
          targets.every((r) => r.category === UNCATEGORIZED && !r.categoryManual),
        onSelect: () =>
          void editThenRefresh(() => clearCategoryScope(current.scope), 'clearCategory', {
            resetSelection: current.scope.kind === 'matching'
          })
      },
      { id: 'd2', label: '', divider: true },
      {
        id: 'reveal',
        label: t('menu.reveal'),
        disabled: !one,
        onSelect: () => one && reveal(one)
      },
      {
        id: 'show-folder',
        label: t('menu.showInFolders'),
        disabled: !one,
        onSelect: () => {
          if (!one) return
          const dir = one.path.slice(
            0,
            Math.max(one.path.lastIndexOf('/'), one.path.lastIndexOf('\\'))
          )
          setFolderFilter(dir === activeLibrary()?.root ? null : dir)
        }
      },
      {
        id: 'copy-path',
        label: one ? t('menu.copyPath') : t('menu.copyPaths', { count: targets.length }),
        onSelect: () => copyPaths(targets)
      }
    ]
  }

  /**
   * The library header's menu: everything that acts on one library.
   *
   * These used to live in the settings dialog, which could only ever act on the
   * active one — the reason they moved here. Right-clicking the header says which
   * library the action applies to, so reindexing a background library no longer
   * means switching to it first.
   */
  const libMenuItems = (): MenuItem[] => {
    const current = libMenu()
    if (!current) return []
    const library = current.library
    const busy = indexing() !== null
    return [
      {
        id: 'switch',
        label: t('libMenu.switch'),
        disabled: library.active,
        onSelect: () => void switchLibrary(library.root)
      },
      { id: 'd1', label: '', divider: true },
      {
        id: 'reindex',
        label: t('libMenu.reindex'),
        disabled: busy,
        onSelect: () => void reindexLibrary(library.root, false)
      },
      {
        id: 'reindex-full',
        label: t('libMenu.fullReindex'),
        disabled: busy,
        onSelect: () => void reindexLibrary(library.root, true)
      },
      { id: 'd2', label: '', divider: true },
      {
        id: 'reveal',
        label: t('menu.reveal'),
        onSelect: () => api.revealInFolder(library.root).catch((e) => setNotice(String(e)))
      },
      {
        id: 'close',
        label: t('libMenu.close'),
        onSelect: () => void removeLibrary(library.root)
      }
    ]
  }

  /**
   * The folder row's menu.
   *
   * Same panel as the two above rather than the OS menu: a native menu does not
   * follow the theme and cannot call back into the app, and the rail is the one
   * place a right-click used to produce something that looked like a different
   * program.
   */
  const folderMenuItems = (): MenuItem[] => {
    const current = folderMenu()
    if (!current) return []
    return [
      {
        id: 'reveal',
        label: t('folderMenu.reveal'),
        onSelect: () => api.revealInFolder(current.path).catch((e) => setNotice(String(e)))
      },
      {
        id: 'filter',
        label: t('folderMenu.filter'),
        onSelect: () => setFolderFilter(current.path)
      },
      {
        id: 'copy-path',
        label: t('folderMenu.copyPath'),
        onSelect: () => {
          void navigator.clipboard.writeText(current.path)
          setNotice(t('notice.pathCopied'))
        }
      }
    ]
  }

  /**
   * The header's right-click menu.
   *
   * One entry per column with a tick slot, then the way into the settings section
   * for the same list. A ticked row toggles the column and closes the menu — the
   * component's own semantics, deliberately not Explorer's keep-open behaviour:
   * one menu should not have two closing rules.
   */
  const headerMenuItems = (): MenuItem[] => {
    const hidden = hiddenColumns()
    return [
      ...COLUMNS.map((column) => ({
        id: `column-${column.key}`,
        label: column.label(),
        checked: !hidden.has(column.key),
        disabled: column.pinned,
        onSelect: () => toggleColumn(column.key)
      })),
      { id: 'columns-divider', label: '', divider: true },
      {
        id: 'columns-settings',
        label: t('colMenu.settings'),
        onSelect: () => {
          setSettingsFocus('columns')
          setSettingsOpen(true)
        }
      }
    ]
  }

  const applyCategory = (name: string, sub: string | null) => {
    const dialog = categoryDialog()
    setCategoryDialog(null)
    if (!dialog) return
    void editThenRefresh(() => applyCategoryScope(dialog.scope, name, sub), 'setCategory', {
      resetSelection: dialog.scope.kind === 'matching'
    })
  }

  const applyUcsCategory = (value: UcsCategory) => {
    const dialog = categoryDialog()
    if (!dialog) return
    setCategoryDialog(null)
    void editThenRefresh(
      () =>
        dialog.scope.kind === 'paths'
          ? api.setUcsCategory(dialog.scope.paths, value)
          : api.setUcsCategoryMatching(
              {
                request: dialog.scope.request,
                exclude: dialog.scope.exclude,
                offset: dialog.scope.offset,
                limit: dialog.scope.limit
              },
              value
            ),
      'setUcsCategory',
      { resetSelection: dialog.scope.kind === 'matching' }
    )
  }

  const clearCategory = () => {
    const dialog = categoryDialog()
    setCategoryDialog(null)
    if (!dialog) return
    void editThenRefresh(() => clearCategoryScope(dialog.scope), 'clearCategory', {
      resetSelection: dialog.scope.kind === 'matching'
    })
  }

  /**
   * Shortcuts that belong to the app rather than the table: Space, Ctrl+O,
   * Ctrl+Z/Y, Ctrl+, and the auto-play toggle. The table owns arrows and Ctrl+A.
   */
  const onKeyDown = (event: KeyboardEvent) => {
    // A native input inside an iX shadow root reports its host as
    // `activeElement`. The shared predicate lists only the hosts that take
    // typing: a focused `ix-button` still owns Space and Enter, but arrows and
    // Ctrl+A must keep working for the table.
    const inField = isTextEntry(focusedElement())

    // Nothing but Escape works while a blocking index pass owns the window.
    if (indexing()) return

    const mod = event.ctrlKey || event.metaKey

    if (mod && event.key.toLowerCase() === 'z' && !inField) {
      event.preventDefault()
      if (event.shiftKey) void redo()
      else void undo()
      return
    }
    if (mod && event.key.toLowerCase() === 'y' && !inField) {
      event.preventDefault()
      void redo()
      return
    }
    if (mod && event.key === ',') {
      event.preventDefault()
      setSettingsOpen(true)
      return
    }
    if (mod && event.key.toLowerCase() === 'o') {
      event.preventDefault()
      void addLibrary()
      return
    }

    if (
      event.code === 'Space' &&
      !inField &&
      !categoryDialog() &&
      !settingsOpen() &&
      !menu() &&
      !libMenu() &&
      !folderMenu()
    ) {
      event.preventDefault()
      void togglePlayback()
      return
    }

    if (event.altKey && event.key.toLowerCase() === 'a' && !inField) {
      event.preventDefault()
      toggleAutoPlay()
      return
    }

    // L toggles the loop. No modifier: it is a transport key, the same class as
    // Space, and a sample auditioning session presses it constantly.
    if (event.key.toLowerCase() === 'l' && !inField && !mod && !event.altKey) {
      event.preventDefault()
      void toggleLoop()
      return
    }

    if (event.key === 'Escape') {
      // A selection is the innermost state on screen, so Escape drops it before it
      // starts closing menus and dialogs. Nothing else in the dock is transient.
      if (region()) {
        changeRegion(null)
        return
      }
      if (folderMenu()) setFolderMenu(null)
      else if (libMenu()) setLibMenu(null)
      else if (menu()) setMenu(null)
      else if (settingsOpen()) setSettingsOpen(false)
    }
  }

  /** Drag the divider to resize the rail; double-click resets it. */
  const startResize = (event: PointerEvent) => {
    event.preventDefault()
    const startX = event.clientX
    const startW = (event.currentTarget as HTMLElement).parentElement?.getBoundingClientRect().width ?? sidebarW()
    const move = (e: PointerEvent) => {
      const next = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startW + e.clientX - startX))
      setSidebarW(next)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      localStorage.setItem(SIDEBAR_KEY, String(sidebarW()))
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  onMount(() => {
    // The theme has to be on the document before the first paint, or the window
    // flashes iX's default palette. `main.tsx` applies it too; this call covers a
    // settings change made in another window.
    setTheme(applyTheme(theme()).id)
    // The scale is a webview property, not a document one: applying it here is the
    // first moment there is a webview to talk to. `App` mirrors the stored value
    // for the picker and never re-applies it on mount, so this is the only call.
    void applyZoom(zoom())
    // Push the remembered level into the engine: the slider is renderer state,
    // and the engine starts at unity.
    void api.setGain(gain()).catch(() => {})
    void getVersion().then(setAppVersion).catch(() => {})
    // Mirror the engine's loop state at startup: the switch is engine state, so
    // the renderer has to read it rather than assume it starts off.
    void api
      .loop()
      .then((state) => setLooping(state.enabled))
      .catch(() => {})
    void refresh()
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  /**
   * The chips over the table, one per active filter.
   *
   * Categories and subcategories get a chip each rather than one chip per list:
   * with several lit, a single chip could only clear all of them, and the point of
   * the multi-select is to drop one kind without losing the rest.
   */
  const filterChips = () => {
    const chips: { label: string; clear: () => void }[] = []
    for (const name of categories()) {
      chips.push({
        label: categoryName(name),
        clear: () => toggleCategory(name, true)
      })
    }
    for (const entry of subcategories()) {
      chips.push({
        label: `${categoryName(entry.category)} / ${categoryName(entry.sub)}`,
        clear: () => toggleSubcategory(entry.category, entry.sub, true)
      })
    }
    for (const name of ucsCategories()) {
      chips.push({
        label: t('side.ucsPrefix', { name }),
        clear: () => toggleUcsCategory(name, true)
      })
    }
    for (const entry of ucsSubcategories()) {
      chips.push({
        label: t('side.ucsSubPrefix', {
          category: entry.category,
          subcategory: entry.sub
        }),
        clear: () => toggleUcsSubcategory(entry.category, entry.sub, true)
      })
    }
    for (const catId of ucsCatIds()) {
      chips.push({
        label: catId,
        clear: () => setUcsCatIds(ucsCatIds().filter((value) => value !== catId))
      })
    }
    if (folder()) {
      chips.push({
        label: folder()!.split(/[\\/]/).pop() ?? folder()!,
        clear: () => setFolderFilter(null)
      })
    }
    if (channelsFilter() !== null) {
      chips.push({
        label: t('search.channels', { value: channelsFilter()! }),
        clear: () => {
          setChannelsFilter(null)
          void runSearch()
        }
      })
    }
    if (sampleRateFilter() !== null) {
      chips.push({
        label: t('search.sampleRate', { value: sampleRateLabel(sampleRateFilter()) }),
        clear: () => {
          setSampleRateFilter(null)
          void runSearch()
        }
      })
    }
    if (bpmFilter() !== null) {
      chips.push({
        label: t('search.bpm', { value: formatBpm(bpmFilter()) }),
        clear: () => {
          setBpmFilter(null)
          void runSearch()
        }
      })
    }
    if (coverFilter() !== null) {
      chips.push({
        label: t('search.sameCover'),
        clear: () => {
          setCoverFilter(null)
          void runSearch()
        }
      })
    }
    if (favoritesOnly()) {
      chips.push({
        label: t('search.favorites'),
        clear: () => {
          setFavoritesOnly(false)
          void runSearch()
        }
      })
    }
    return chips
  }

  return (
    <div class="app">
      <header class="topbar">
        <div class="brand">
          <img class="brand-mark" src={brandLogo} alt="" aria-hidden="true" />
          <span class="brand-name">Mio Sound Manager</span>
        </div>

        <div class="spacer" />

        <Show when={matching() > 0}>
          <span class="dim small match-count" id="match-count">
            {matching() === 1
              ? t('search.matchesOne')
              : t('search.matches', { count: matching().toLocaleString() })}
          </span>
        </Show>
        <ix-button
          id="add-library"
          class="slim-button"
          variant="secondary"
          icon={ICON.folderOpen}
          onClick={() => void addLibrary()}
          title={t('topbar.addLibrary')}
        >
          {t('search.addLibrary')}
        </ix-button>

        <Show when={history().undo}>
          <ix-icon-button
            id="undo-button"
            size="16"
            variant="tertiary"
            icon={ICON.undo}
            title={t('topbar.undo', { label: editKindName(history().undo!) })}
            aria-label={t('topbar.undoLabel')}
            onClick={() => void undo()}
          />
        </Show>
        <Show when={history().redo}>
          <ix-icon-button
            id="redo-button"
            size="16"
            variant="tertiary"
            icon={ICON.redo}
            title={t('topbar.redo', { label: editKindName(history().redo!) })}
            aria-label={t('topbar.redoLabel')}
            onClick={() => void redo()}
          />
        </Show>

        <Show when={notice()}>
          <button class="notice" onClick={() => setNotice(null)} title={notice() ?? t('common.dismiss')}>
            {notice()}
          </button>
        </Show>

        <ix-icon-button
          id="settings-button"
          size="16"
          variant="tertiary"
          icon={ICON.settings}
          title={t('topbar.settings')}
          aria-label={t('topbar.settingsLabel')}
          onClick={() => setSettingsOpen(true)}
        />
      </header>

      <div class="body" style={{ '--mio-rail-width': `${sidebarW()}px` }}>
        {/*
          The rail and its drag handle share a wrapper so the handle can sit on the
          rail's own edge. It cannot be a grid child of `.body`: that would add a
          third column and push the table sideways.
        */}
        <div class="rail">
          <Sidebar
            libraries={libraries()}
            facets={facets()}
            ucsFacets={ucsFacets()}
            ucsCatalog={ucsCatalog()}
            favorites={favorites()}
            total={total()}
            folders={folders()}
            categories={categories()}
            subcategories={subcategories()}
            ucsCategories={ucsCategories()}
            ucsSubcategories={ucsSubcategories()}
            ucsCatIds={ucsCatIds()}
            mode={sidebarMode()}
            folder={folder()}
            favoritesOnly={favoritesOnly()}
            onActivate={(root) => void switchLibrary(root)}
            onCategory={toggleCategory}
            onSubcategory={toggleSubcategory}
            onUcsCategory={toggleUcsCategory}
            onUcsSubcategory={toggleUcsSubcategory}
            onMode={changeSidebarMode}
            onFolder={setFolderFilter}
            onFolderMenu={(request) => setFolderMenu(request)}
            onToggleFavorites={() => {
              setFavoritesOnly(!favoritesOnly())
              void runSearch()
            }}
            onOpenSettings={() => setSettingsOpen(true)}
            onLibraryMenu={(library, x, y) => setLibMenu({ library, x, y })}
          />
          <div class="rail-resize" onPointerDown={startResize} title={t('side.resize')} />
        </div>

        <main class="main">
          <div class="main-head">
            <div class="search">
              <ix-input
                id="search-input"
                placeholder={t('search.placeholder')}
                value={searchDraft()}
                on:valueChange={(e: CustomEvent<string>) => setSearchDraft(e.detail)}
                onKeyDown={(e: KeyboardEvent) => {
                  if (e.isComposing || e.keyCode === 229) return
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    commitSearchTerm()
                  } else if (e.key === 'Escape') {
                    if (searchDraft()) setSearchDraft('')
                    else clearSearchTerms()
                  }
                }}
              />
            </div>
          {queryTerms().map((term) => (
            <ix-chip
              class="filter-chip query-chip"
              data-term={term}
              variant="primary"
              outline
              closable
              onClick={() => removeSearchTerm(term)}
              on:closeChip={() => removeSearchTerm(term)}
            >
              {term}
            </ix-chip>
          ))}
          {filterChips().map((chip) => (
            <ix-chip
              class="filter-chip"
              variant="primary"
              outline
              closable
              onClick={chip.clear}
              on:closeChip={chip.clear}
            >
              {chip.label}
            </ix-chip>
          ))}
        </div>

          <Show
            when={libraries().length > 0}
            fallback={
              <div class="empty">
                <ix-icon name={bootError() ? ICON.error : ICON.database} size="48" />
                <h1>{bootError() ? t('empty.backendError') : t('empty.noLibrary')}</h1>
                <Show when={bootError()}>
                  <p class="bad">{bootError()}</p>
                </Show>
                <Show when={!bootError()}>
                  <p>{t('empty.body')}</p>
                </Show>
                <ix-button variant="primary" icon={ICON.folderOpen} onClick={() => void addLibrary()}>
                  {t('empty.open')}
                </ix-button>
              </div>
            }
          >
            <div class="main-body">
              <SoundTable
                epoch={dataEpoch()}
                requestKey={JSON.stringify(request())}
                selectionReset={selectionReset()}
                request={request()}
                fetchWindow={fetchWindow}
                active={active()}
                sortKey={sortKey()}
                sortAsc={sortAsc()}
                matching={matching()}
                querying={querying()}
                favoriteOverride={favoriteOverride()}
                onSort={changeSort}
                onSelectionChange={changeSelection}
                onActivate={activate}
                onRowMenu={setMenu}
                onToggleFavorite={toggleFavorite}
                onSelectAll={onSelectAll}
                onHeaderMenu={(x, y) => setHeaderMenu({ x, y })}
                coverEpoch={coverEpoch()}
                onCoverSearch={showCoverExclusive}
              />
              <Inspector
                row={active()}
                selectionCount={selectionCount()}
                open={inspectorOpen()}
                width={inspectorW()}
                coverEpoch={coverEpoch()}
                onOpen={() => {
                  setInspectorOpen(true)
                  localStorage.setItem(INSPECTOR_OPEN_KEY, '1')
                }}
                onClose={() => {
                  setInspectorOpen(false)
                  localStorage.setItem(INSPECTOR_OPEN_KEY, '0')
                }}
                onResize={changeInspectorWidth}
                onQueryTerm={addQueryTerm}
                onCopy={copyValue}
                onFolder={showFolderExclusive}
                onCover={showCoverExclusive}
                onCategory={showCategoryExclusive}
                onSubcategory={showSubcategoryExclusive}
                onUcsCategory={showUcsCategoryExclusive}
                onUcsSubcategory={showUcsSubcategoryExclusive}
                onUcsCatId={showUcsCatIdExclusive}
                onChannels={toggleChannelsFilter}
                onSampleRate={toggleSampleRateFilter}
                onBpm={toggleBpmFilter}
              />
            </div>
          </Show>

          <WaveformDock
            row={active()}
            positionS={playhead.positionS()}
            durationS={playhead.durationS()}
            playing={playhead.playing()}
            paused={playhead.paused()}
            selectionCount={selectionCount()}
            autoPlay={settings().autoPlay}
            gain={gain()}
            region={region()}
            looping={looping()}
            exporting={exportingRegion()}
            onStop={() => void stop()}
            onToggle={() => void togglePlayback()}
            onToggleAutoPlay={toggleAutoPlay}
            onToggleMute={toggleMute}
            onToggleLoop={() => void toggleLoop()}
            onGain={changeGain}
            onSeek={(seconds) => {
              const row = active()
              if (row) void play(row, seconds)
            }}
            onRegion={changeRegion}
            onDragOut={(atSeconds) => void dragRegionOut(atSeconds)}
          />
        </main>
      </div>

      <footer class="statusbar">
        <Show
          when={status()?.audioAvailable}
          fallback={<span class="bad status-device">{status()?.audioError ?? t('status.noDevice')}</span>}
        >
          <span class="good">●</span>
          <span class="backend-tag">{status()?.backend}</span>
          <span class="status-device" title={status()?.deviceName ?? undefined}>{status()?.deviceName}</span>
          <span class="dim">
            {formatSampleRate(status()?.sampleRate ?? null)} · {status()?.channels} ch · {status()?.sampleFormat}
          </span>
        </Show>

        <div class="spacer" />

        <Show when={selectionCount() > 1}>
          <span class="dim">{t('status.selected', { count: selectionCount().toLocaleString() })}</span>
        </Show>

        <Show when={metadataLibrary()}>
          {(library) => (
            <div class="metadata-progress" role="status" aria-live="polite">
              <span id="metadata-progress-label">{t('index.metadata', {
                done: library().metadataIndexed.toLocaleString(),
                total: library().metadataTotal.toLocaleString()
              })}</span>
              <progress
                aria-labelledby="metadata-progress-label"
                max={Math.max(1, library().metadataTotal)}
                /* Rows dealt with: indexed, plus the ones whose file changed
                   under the pass and the ones that failed. A bar that counted
                   only `indexed` would stop short of 100% forever, and one that
                   counted a changed row as indexed claimed work it never did. */
                value={
                  library().metadataIndexed +
                  library().metadataDeferred +
                  library().metadataFailed
                }
              />
              <Show when={library().metadataFailed > 0}>
                <span class="dim">{t('index.metadataFailed', { count: library().metadataFailed })}</span>
              </Show>
            </div>
          )}
        </Show>

        <Show when={appVersion()}>
          {(version) => <span class="dim status-version">v{version()}</span>}
        </Show>
      </footer>

      <Show when={menu()}>
        {(current) => (
          <ContextMenu
            x={current().x}
            y={current().y}
            items={menuItems()}
            onClose={() => setMenu(null)}
          />
        )}
      </Show>

      <Show when={libMenu()}>
        {(current) => (
          <ContextMenu
            x={current().x}
            y={current().y}
            items={libMenuItems()}
            onClose={() => setLibMenu(null)}
          />
        )}
      </Show>

      <Show when={folderMenu()}>
        {(current) => (
          <ContextMenu
            x={current().x}
            y={current().y}
            items={folderMenuItems()}
            onClose={() => setFolderMenu(null)}
          />
        )}
      </Show>

      <Show when={headerMenu()}>
        {(current) => (
          <ContextMenu
            x={current().x}
            y={current().y}
            items={headerMenuItems()}
            onClose={() => setHeaderMenu(null)}
          />
        )}
      </Show>

      <Show when={categoryDialog()}>
        {(dialog) => (
          <CategoryDialog
            count={dialog().count}
            facets={facets()}
            ucsCategories={ucsCatalog()}
            onSave={applyCategory}
            onSaveUcs={applyUcsCategory}
            onClear={clearCategory}
            onClose={() => setCategoryDialog(null)}
          />
        )}
      </Show>

      <Show when={settingsOpen()}>
        <SettingsDialog
          settings={settings()}
          theme={theme()}
          zoom={zoom()}
          focus={settingsFocus()}
          onChange={changeSettings}
          onTheme={changeTheme}
          onZoom={changeZoom}
          status={status()}
          libraries={libraries()}
          activeLibrary={activeLibrary()}
          onStatus={(next) => {
            // A device switch changes the format, so anything derived from the old
            // one has to be re-read: the rows are fine, but the playhead, the
            // status bar and the dock all read the new engine.
            setStatus(next)
          }}
          onAddLibrary={() => void addLibrary()}
          onClose={() => {
            setSettingsOpen(false)
            setSettingsFocus(undefined)
          }}
        />
      </Show>

      <IndexOverlay
        open={indexing() !== null}
        label={indexing()?.label ?? ''}
        progress={indexing()?.progress ?? null}
      />
    </div>
  )
}
