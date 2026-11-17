import { Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import {
  api,
  type AppStatus,
  type EditKind,
  type Facet,
  type FolderNode,
  type IndexProgress,
  type LibraryStatus,
  type SoundRow
} from './ipc'
import { SoundTable, DEFAULT_DIR, SORT_WIRE, type RowMenuRequest, type SortKey } from './SoundTable'
import { WaveformDock } from './WaveformDock'
import { Sidebar } from './Sidebar'
import { ContextMenu, type MenuItem } from './ContextMenu'
import { CategoryDialog } from './CategoryDialog'
import { SettingsDialog, loadSettings, saveSettings, type Settings } from './SettingsDialog'
import { IndexOverlay } from './IndexOverlay'
import { UNCATEGORIZED, editKindName, t } from './i18n'
import { ICON } from './icons'
import { applyTheme, loadThemeId } from './theme'

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

  let frame = 0
  let inFlight = false
  let stopped = false

  const tick = async () => {
    if (stopped) return
    if (!inFlight) {
      inFlight = true
      try {
        const p = await api.position()
        setPositionS(p.positionS)
        setDurationS(p.durationS)
        setPlaying(p.playing)
        setPaused(p.paused)
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

  return { positionS, durationS, playing, paused }
}

/** Sidebar width, remembered per machine. Clamped to what the layout can take. */
const SIDEBAR_KEY = 'mio:sidebar-width'
const SIDEBAR_MIN = 180
const SIDEBAR_MAX = 520
/** Output gain, remembered per machine. */
const GAIN_KEY = 'mio:gain'

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

  const [status, setStatus] = createSignal<AppStatus | null>(null)
  const [bootError, setBootError] = createSignal<string | null>(null)
  const [libraries, setLibraries] = createSignal<LibraryStatus[]>([])
  const [rows, setRows] = createSignal<SoundRow[]>([])
  const [matching, setMatching] = createSignal(0)
  const [folders, setFolders] = createSignal<FolderNode | null>(null)
  const [facets, setFacets] = createSignal<Facet[]>([])
  const [favorites, setFavorites] = createSignal(0)
  const [total, setTotal] = createSignal(0)

  const [query, setQuery] = createSignal('')
  const [category, setCategory] = createSignal<string | null>(null)
  const [subcategory, setSubcategory] = createSignal<string | null>(null)
  const [folder, setFolder] = createSignal<string | null>(null)
  const [favoritesOnly, setFavoritesOnly] = createSignal(false)

  const [sortKey, setSortKey] = createSignal<SortKey>('filename')
  const [sortAsc, setSortAsc] = createSignal(true)

  const [selection, setSelection] = createSignal<string[]>([])
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
  const [categoryDialog, setCategoryDialog] = createSignal<SoundRow[] | null>(null)
  const [settingsOpen, setSettingsOpen] = createSignal(false)
  const [history, setHistory] = createSignal<{ undo: EditKind | null; redo: EditKind | null }>({
    undo: null,
    redo: null
  })

  const [notice, setNotice] = createSignal<string | null>(null)
  const [waveMs, setWaveMs] = createSignal<number | null>(null)

  /** A blocking index pass is running; the window is locked behind the overlay. */
  const [indexing, setIndexing] = createSignal<{ label: string; progress: IndexProgress | null } | null>(
    null
  )
  /** Guards against a second `onLoadMore` while one is in flight. */
  const [loadingMore, setLoadingMore] = createSignal(false)
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
  const [gain, setGain] = createSignal(
    Math.min(1, Math.max(0, Number(localStorage.getItem(GAIN_KEY)) || 1))
  )

  const playhead = usePlayhead()

  const activeLibrary = createMemo(() => libraries().find((l) => l.active) ?? null)
  const hasMore = createMemo(() => rows().length < matching())

  /**
   * The search request, derived once so the search call, "select all" and the
   * next page cannot drift apart.
   *
   * `sortBy` is the index's wire name, not the table's field name: see
   * `SORT_WIRE`. Sending `durationS` there is what made every search after a
   * Length click fail.
   */
  const request = createMemo(() => ({
    query: query(),
    category: category() ?? undefined,
    subcategory: subcategory() ?? undefined,
    folder: folder() ?? undefined,
    favoriteOnly: favoritesOnly() ? true : undefined,
    sortBy: SORT_WIRE[sortKey()],
    sortDir: (sortAsc() ? 'asc' : 'desc') as 'asc' | 'desc'
  }))

  /** Replace the list with the first page. */
  const runSearch = async () => {
    if (libraries().length === 0) {
      setRows([])
      setMatching(0)
      setQuerying(false)
      return
    }
    const req = { ...request(), limit: settings().pageSize, offset: 0 }
    setQuerying(true)
    try {
      const [page, count] = await Promise.all([api.search(req), api.countMatching(request())])
      setRows(page)
      setMatching(count)
    } catch (e) {
      setNotice(String(e))
    } finally {
      setQuerying(false)
    }
  }

  /**
   * Append the next page.
   *
   * Offset is the number of rows already loaded, which is correct because the
   * request is sorted: the index returns a stable window for a given sort, so page
   * two cannot overlap page one. A re-sort resets the list, so a stale offset
   * cannot survive into a different ordering.
   */
  const loadMore = async () => {
    if (loadingMore() || !hasMore()) return
    setLoadingMore(true)
    try {
      const req = { ...request(), limit: settings().pageSize, offset: rows().length }
      const page = await api.search(req)
      if (page.length === 0) {
        // Nothing came back: the count is ahead of the data (a pass finished
        // between the two queries). Trusting the count would keep asking forever.
        setMatching(rows().length)
      } else {
        setRows([...rows(), ...page])
      }
    } catch (e) {
      setNotice(String(e))
    } finally {
      setLoadingMore(false)
    }
  }

  // Coalesce keystrokes at ~80ms. The index answers a search in ~2ms, so the only
  // thing an unthrottled call buys is a table that flickers through intermediate
  // result sets while a word is being typed.
  let searchTimer = 0
  const scheduleSearch = () => {
    clearTimeout(searchTimer)
    // Raised here, not only inside `runSearch`, so the debounce window is not a
    // gap where the table looks settled while a query is already queued.
    setQuerying(true)
    searchTimer = window.setTimeout(() => void runSearch(), 80)
  }

  const loadTree = async () => {
    try {
      setFolders(await api.folders())
    } catch {
      setFolders(null)
    }
  }

  /**
   * Facet counts. Refreshed after every library edit rather than on a timer: a
   * stale count next to a category the user just emptied is the kind of wrong
   * number that makes the whole rail untrustworthy.
   */
  const loadSidebar = async () => {
    try {
      const sidebar = await api.sidebar()
      setFacets(sidebar.facets)
      setFavorites(sidebar.favorites)
      setTotal(sidebar.total)
    } catch {
      setFacets([])
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
      setLibraries(await api.listLibraries())
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
    setWaveMs(null)
    try {
      await api.play(row.path, startAt)
    } catch (e) {
      setNotice(String(e))
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
      setNotice(String(e))
    }
  }

  /**
   * Move the output level.
   *
   * The signal is set first so the slider and the percentage follow the pointer
   * with no latency; the engine call is fire-and-forget because a dropped one
   * costs a level change the user will make again, not a broken state. The value
   * is persisted so the next launch starts where this one ended.
   */
  const changeGain = (next: number) => {
    const clamped = Math.min(1, Math.max(0, next))
    setGain(clamped)
    localStorage.setItem(GAIN_KEY, String(clamped))
    api.setGain(clamped).catch(() => {
      // The engine clamps to its own range; a rejection here means no device,
      // and the level is remembered for whenever one opens.
    })
  }

  /**
   * Add a library. Blocks on the index pass and shows the overlay while it runs.
   *
   * `add_library` only resolves when the pass is done, so the overlay is driven by
   * the promise lifetime, not by a separate poll: there is no window in which the
   * app thinks it is idle while the database is still being written.
   */
  const addLibrary = async (root?: string) => {
    let target = root
    if (!target) {
      const picked = await api.pickLibraryFolder()
      if (!picked) return
      target = picked
    }

    setIndexing({ label: t('index.indexing'), progress: null })
    try {
      await api.addLibrary(target, (progress) => {
        setIndexing({ label: t('index.indexing'), progress })
      })
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
      await api.reindexLibrary(root, (progress) => {
        setIndexing({ label: full ? t('index.fullReindex') : t('index.reindexing'), progress })
      }, full)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setIndexing(null)
    }
  }

  const switchLibrary = async (root: string) => {
    if (activeLibrary()?.root === root) return
    try {
      setLibraries(await api.setActiveLibrary(root))
      // A different database means every derived thing is stale: the facets, the
      // folder tree, the undo stack and the rows.
      setCategory(null)
      setSubcategory(null)
      setFolder(null)
      setFavoritesOnly(false)
      setSelection([])
      setActive(null)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const removeLibrary = async (root: string) => {
    try {
      setLibraries(await api.removeLibrary(root))
      await refresh()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const changeSettings = (patch: Partial<Settings>) => {
    const next = { ...settings(), ...patch }
    setSettings(next)
    saveSettings(next)
    if (patch.autoPlay !== undefined) {
      // The dock's AUTO PLAY switch and this setting are the same policy, so they
      // are persisted under the same key rather than two that can disagree.
      localStorage.setItem('mio:autoplay', String(next.autoPlay))
    }
    if (patch.pageSize !== undefined || patch.infiniteScroll !== undefined) {
      void runSearch()
    }
  }

  const changeTheme = (id: string) => {
    setTheme(applyTheme(id).id)
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

  const changeSelection = (paths: string[], nextActive: SoundRow | null) => {
    setSelection(paths)
    if (nextActive) setActive(nextActive)
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

  const setCategoryFilter = (name: string | null, sub: string | null) => {
    setCategory(name)
    setSubcategory(sub)
    // A category and a folder are two ways of narrowing the same list; keeping
    // both would produce an empty result the user cannot explain.
    setFolder(null)
    void runSearch()
  }

  const setFolderFilter = (path: string | null) => {
    setFolder(path)
    setCategory(null)
    setSubcategory(null)
    setFavoritesOnly(false)
    void runSearch()
  }

  /**
   * Select every path the filter matches.
   *
   * Deliberately a separate command from `search`: the page on screen is clamped,
   * and "select all" in a filtered subset of a 280k library is not a page. The
   * backend returns paths only, which is why this does not need full rows.
   */
  const selectAll = async () => {
    try {
      const paths = await api.matchingPaths(request())
      setSelection(paths)
      setNotice(t('notice.selected', { count: paths.length.toLocaleString() }))
    } catch (e) {
      setNotice(String(e))
    }
  }

  /** Apply a library edit, then refresh the counts, the rows and the undo labels. */
  const editThenRefresh = async (paths: string[], apply: () => Promise<unknown>) => {
    if (paths.length === 0) return
    try {
      await apply()
      await Promise.all([loadSidebar(), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  }

  const undo = async () => {
    try {
      const kind = await api.undo()
      if (!kind) return
      setNotice(t('notice.undid', { label: editKindName(kind) }))
      await Promise.all([loadSidebar(), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  }

  const redo = async () => {
    try {
      const kind = await api.redo()
      if (!kind) return
      setNotice(t('notice.redid', { label: editKindName(kind) }))
      await Promise.all([loadSidebar(), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  }

  const toggleFavorite = (row: SoundRow) => {
    void editThenRefresh([row.path], () => api.setFavorite([row.path], !row.favorite))
  }

  const toggleFavoriteSelection = () => {
    const targets = menu()?.targets ?? []
    if (targets.length === 0) return
    const allStarred = targets.every((r) => r.favorite)
    void editThenRefresh(
      targets.map((r) => r.path),
      () => api.setFavorite(targets.map((r) => r.path), !allStarred)
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
    const one = targets.length === 1 ? targets[0] : null
    const many = (label: string) => (one ? label : t('menu.withCount', { label, count: targets.length }))
    const allStarred = targets.every((r) => r.favorite)

    return [
      {
        id: 'play',
        label: t('menu.play'),
        shortcut: 'Space',
        disabled: !one,
        onSelect: () => one && void play(one)
      },
      { id: 'd1', label: '', divider: true },
      {
        id: 'favorite',
        label: many(allStarred ? t('edit.unfavorite') : t('edit.favorite')),
        onSelect: toggleFavoriteSelection
      },
      {
        id: 'category',
        label: many(t('menu.setCategory')),
        onSelect: () => setCategoryDialog(targets)
      },
      {
        id: 'clear-category',
        label: many(t('menu.clearCategory')),
        disabled: targets.every((r) => r.category === UNCATEGORIZED && !r.categoryManual),
        onSelect: () =>
          void editThenRefresh(
            targets.map((r) => r.path),
            () => api.clearCategory(targets.map((r) => r.path))
          )
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
          setCategoryFilter(null, null)
          setFolderFilter(dir === activeLibrary()?.root ? null : dir)
        }
      },
      {
        id: 'copy-path',
        label: one ? t('menu.copyPath') : t('menu.copyPaths', { count: targets.length }),
        onSelect: () => copyPaths(targets)
      },
      { id: 'd3', label: '', divider: true },
      {
        id: 'drag',
        label: one ? t('menu.dragToDaw') : t('menu.dragToDawMany', { count: targets.length }),
        disabled: true,
        // Dragging is done with the mouse; the menu entry exists so the feature is
        // discoverable, and it says how rather than pretending to be clickable.
        shortcut: t('menu.dragHint')
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

  const applyCategory = (name: string, sub: string | null) => {
    const targets = categoryDialog() ?? []
    setCategoryDialog(null)
    void editThenRefresh(
      targets.map((r) => r.path),
      () => api.setCategory(targets.map((r) => r.path), name, sub)
    )
  }

  const clearCategory = () => {
    const targets = categoryDialog() ?? []
    setCategoryDialog(null)
    void editThenRefresh(
      targets.map((r) => r.path),
      () => api.clearCategory(targets.map((r) => r.path))
    )
  }

  /**
   * Shortcuts that belong to the app rather than the table: Space, Ctrl+O,
   * Ctrl+Z/Y, Ctrl+, and the auto-play toggle. The table owns arrows and Ctrl+A.
   */
  const onKeyDown = (event: KeyboardEvent) => {
    const el = document.activeElement as HTMLElement | null
    const tag = el?.tagName ?? ''
    // iX components put a native input inside a shadow root, so the host is what
    // `activeElement` reports. Treating the whole iX namespace as "in a field"
    // would also swallow Space on a focused button, which is correct: a focused
    // button activates on Space.
    const inField =
      tag === 'INPUT' ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      tag === 'IX-INPUT' ||
      tag === 'IX-SLIDER' ||
      tag === 'IX-SELECT' ||
      el?.isContentEditable === true

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
      !libMenu()
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

    if (event.key === 'Escape') {
      if (libMenu()) setLibMenu(null)
      else if (menu()) setMenu(null)
      else if (settingsOpen()) setSettingsOpen(false)
    }
  }

  /** Drag the divider to resize the rail; double-click resets it. */
  const startResize = (event: PointerEvent) => {
    event.preventDefault()
    const startX = event.clientX
    const startW = sidebarW()
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
    // Push the remembered level into the engine: the slider is renderer state,
    // and the engine starts at unity.
    void api.setGain(gain()).catch(() => {})
    void refresh()
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  const filterChips = () => {
    const chips: { label: string; clear: () => void }[] = []
    if (category()) {
      chips.push({
        label: subcategory() ? `${category()} / ${subcategory()}` : category()!,
        clear: () => setCategoryFilter(null, null)
      })
    }
    if (folder()) {
      chips.push({
        label: folder()!.split(/[\\/]/).pop() ?? folder()!,
        clear: () => setFolderFilter(null)
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
          <span class="brand-name">Mio Sound Manager</span>
          <span class="brand-sub">{t('app.subtitle')}</span>
        </div>

        <Show when={activeLibrary()}>
          {(library) => (
            <ix-pill class="lib-chip" variant="neutral" outline title={library().root}>
              {library().name}
            </ix-pill>
          )}
        </Show>

        <div class="spacer" />

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
          <button class="notice" onClick={() => setNotice(null)} title={t('common.dismiss')}>
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

      <div class="body" style={{ 'grid-template-columns': `${sidebarW()}px 1fr` }}>
        {/*
          The rail and its drag handle share a wrapper so the handle can sit on the
          rail's own edge. It cannot be a grid child of `.body`: that would add a
          third column and push the table sideways.
        */}
        <div class="rail">
          <Sidebar
            libraries={libraries()}
            facets={facets()}
            favorites={favorites()}
            total={total()}
            folders={folders()}
            category={category()}
            subcategory={subcategory()}
            folder={folder()}
            favoritesOnly={favoritesOnly()}
            onActivate={(root) => void switchLibrary(root)}
            onCategory={setCategoryFilter}
            onFolder={setFolderFilter}
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
                value={query()}
                on:valueChange={(e: CustomEvent<string>) => {
                  setQuery(e.detail)
                  scheduleSearch()
                }}
                onKeyDown={(e: KeyboardEvent) => {
                  if (e.key === 'Escape') {
                    setQuery('')
                    void runSearch()
                  }
                }}
              />
            </div>
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
            <div class="spacer" />
            <Show when={matching() > 0}>
              <span class="dim small">
                {matching() === 1
                  ? t('search.matchesOne')
                  : t('search.matches', { count: matching().toLocaleString() })}
              </span>
            </Show>
            <ix-button
              variant="secondary"
              icon={ICON.folderOpen}
              onClick={() => void addLibrary()}
              title={t('topbar.addLibrary')}
            >
              {t('search.addLibrary')}
            </ix-button>
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
            <SoundTable
              rows={rows()}
              selection={selection()}
              active={active()}
              sortKey={sortKey()}
              sortAsc={sortAsc()}
              showCategory={settings().showCategory}
              matching={matching()}
              hasMore={hasMore() && settings().infiniteScroll}
              querying={querying()}
              onSort={changeSort}
              onSelectionChange={changeSelection}
              onActivate={activate}
              onRowMenu={setMenu}
              onToggleFavorite={toggleFavorite}
              onSelectAll={() => void selectAll()}
              onLoadMore={() => void loadMore()}
            />
          </Show>

          <WaveformDock
            row={active()}
            positionS={playhead.positionS()}
            durationS={playhead.durationS()}
            playing={playhead.playing()}
            paused={playhead.paused()}
            selectionCount={selection().length}
            autoPlay={settings().autoPlay}
            gain={gain()}
            onStop={() => void stop()}
            onToggle={() => void togglePlayback()}
            onToggleAutoPlay={toggleAutoPlay}
            onGain={changeGain}
            onSeek={(seconds) => {
              const row = active()
              if (row) void play(row, seconds)
            }}
            onDecoded={setWaveMs}
          />
        </main>
      </div>

      <footer class="statusbar">
        <Show
          when={status()?.audioAvailable}
          fallback={<span class="bad">{status()?.audioError ?? t('status.noDevice')}</span>}
        >
          <span class="good">●</span>
          <span class="backend-tag">{status()?.backend}</span>
          <span>{status()?.deviceName}</span>
          <span class="dim">
            {status()?.sampleRate} Hz · {status()?.channels} ch · {status()?.sampleFormat}
          </span>
          <span class="dim">
            {t('status.enginePeriod', { ms: status()?.enginePeriodMs?.toFixed(2) ?? '—' })}
          </span>
        </Show>

        <div class="spacer" />

        <Show when={selection().length > 1}>
          <span class="dim">{t('status.selected', { count: selection().length.toLocaleString() })}</span>
        </Show>

        <Show when={libraries().length > 0}>
          <span class="dim">
            {t('status.loaded', {
              loaded: rows().length.toLocaleString(),
              matching: matching().toLocaleString()
            })}
          </span>
        </Show>

        <Show when={waveMs()}>
          {(ms) => <span class="dim">{t('status.waveform', { ms: ms().toFixed(1) })}</span>}
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

      <Show when={categoryDialog()}>
        {(targets) => (
          <CategoryDialog
            count={targets().length}
            facets={facets()}
            onSave={applyCategory}
            onClear={clearCategory}
            onClose={() => setCategoryDialog(null)}
          />
        )}
      </Show>

      <Show when={settingsOpen()}>
        <SettingsDialog
          settings={settings()}
          theme={theme()}
          onChange={changeSettings}
          onTheme={changeTheme}
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
          onClose={() => setSettingsOpen(false)}
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
