import { Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import {
  api,
  type AppStatus,
  type Facet,
  type FolderNode,
  type IndexProgress,
  type LibraryStatus,
  type SoundRow
} from './ipc'
import { SoundTable, DEFAULT_DIR, type RowMenuRequest, type SortKey } from './SoundTable'
import { WaveformDock } from './WaveformDock'
import { Sidebar } from './Sidebar'
import { ContextMenu, type MenuItem } from './ContextMenu'
import { CategoryDialog } from './CategoryDialog'
import { SettingsDialog, loadSettings, saveSettings, type Settings } from './SettingsDialog'
import { IndexOverlay } from './IndexOverlay'

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
const SIDEBAR_KEY = 'reslip:sidebar-width'
const SIDEBAR_MIN = 160
const SIDEBAR_MAX = 520

export function App() {
  const [settings, setSettings] = createSignal<Settings>(loadSettings())

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
  const [categoryDialog, setCategoryDialog] = createSignal<SoundRow[] | null>(null)
  const [settingsOpen, setSettingsOpen] = createSignal(false)
  const [history, setHistory] = createSignal<{ undo: string | null; redo: string | null }>({
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

  const [sidebarW, setSidebarW] = createSignal(
    Math.min(
      SIDEBAR_MAX,
      Math.max(SIDEBAR_MIN, Number(localStorage.getItem(SIDEBAR_KEY)) || 232)
    )
  )

  const playhead = usePlayhead()

  const activeLibrary = createMemo(() => libraries().find((l) => l.active) ?? null)
  const hasMore = createMemo(() => rows().length < matching())

  /**
   * The search request, derived once so the search call, "select all" and the
   * next page cannot drift apart.
   */
  const request = createMemo(() => ({
    query: query(),
    category: category() ?? undefined,
    subcategory: subcategory() ?? undefined,
    folder: folder() ?? undefined,
    favoriteOnly: favoritesOnly() ? true : undefined,
    sortBy: sortKey(),
    sortDir: (sortAsc() ? 'asc' : 'desc') as 'asc' | 'desc'
  }))

  /** Replace the list with the first page. */
  const runSearch = async () => {
    if (libraries().length === 0) {
      setRows([])
      setMatching(0)
      return
    }
    const req = { ...request(), limit: settings().pageSize, offset: 0 }
    try {
      const [page, count] = await Promise.all([api.search(req), api.countMatching(request())])
      setRows(page)
      setMatching(count)
    } catch (e) {
      setNotice(String(e))
    }
  }

  /**
   * Append the next page.
   *
   * Offset is the number of rows already loaded, which is correct because the
   * request is sorted: the index returns a stable window for a given sort, so
   * page two cannot overlap page one. A re-sort resets the list, so a stale
   * offset cannot survive into a different ordering.
   */
  const loadMore = async () => {
    if (loadingMore() || !hasMore()) return
    setLoadingMore(true)
    try {
      const req = {
        ...request(),
        limit: settings().pageSize,
        offset: rows().length
      }
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
      setHistory({ undo: state.undoLabel, redo: state.redoLabel })
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

    setIndexing({ label: 'Indexing library…', progress: null })
    try {
      await api.addLibrary(target, (progress) => {
        setIndexing({ label: 'Indexing library…', progress })
      })
      setSettingsOpen(false)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setIndexing(null)
    }
  }

  const reindex = async (full: boolean) => {
    setIndexing({
      label: full ? 'Re-probing every file…' : 'Looking for changes…',
      progress: null
    })
    try {
      await api.reindexLibrary((progress) => {
        setIndexing({
          label: full ? 'Re-probing every file…' : 'Looking for changes…',
          progress
        })
      }, full)
      setSettingsOpen(false)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setIndexing(null)
    }
  }

  const switchLibrary = async (root: string) => {
    try {
      setLibraries(await api.setActiveLibrary(root))
      setCategory(null)
      setSubcategory(null)
      setFolder(null)
      setFavoritesOnly(false)
      await Promise.all([loadTree(), loadSidebar()])
      await runSearch()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const removeLibrary = async (root: string) => {
    try {
      setLibraries(await api.removeLibrary(root))
      await Promise.all([loadTree(), loadSidebar()])
      await runSearch()
    } catch (e) {
      setNotice(String(e))
    }
  }

  const changeSelection = (paths: string[], next: SoundRow | null) => {
    setSelection(paths)
    if (next) setActive(next)
  }

  const activate = (row: SoundRow) => {
    if (settings().autoPlay) void play(row)
    else setActive(row)
  }

  const changeSettings = (next: Settings) => {
    setSettings(next)
    saveSettings(next)
  }

  const toggleAutoPlay = () => changeSettings({ ...settings(), autoPlay: !settings().autoPlay })

  const changeSort = (key: SortKey) => {
    if (sortKey() === key) {
      setSortAsc(!sortAsc())
    } else {
      setSortKey(key)
      setSortAsc(DEFAULT_DIR[key] === 'asc')
    }
    void runSearch()
  }

  const setCategoryFilter = (next: string | null, sub: string | null) => {
    setCategory(next)
    setSubcategory(sub)
    void runSearch()
  }

  const setFolderFilter = (next: string | null) => {
    setFolder(next)
    void runSearch()
  }

  /**
   * Ctrl+A. Asks for paths only, because selecting everything in a 280k library
   * is not a table page: `search` clamps at 5,000 rows by design, and a "select
   * all" that silently stops at the clamp is worse than no shortcut.
   */
  const selectAll = async () => {
    try {
      const paths = await api.matchingPaths({ ...request(), limit: 0 })
      setSelection(paths)
      if (paths[0]) {
        const first = rows().find((r) => r.path === paths[0])
        if (first) setActive(first)
      }
      setNotice(`${paths.length.toLocaleString()} selected`)
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
      const label = await api.undo()
      if (!label) return
      setNotice(`Undid ${label.toLowerCase()}`)
      await Promise.all([loadSidebar(), runSearch(), loadHistory()])
    } catch (e) {
      setNotice(String(e))
    }
  }

  const redo = async () => {
    try {
      const label = await api.redo()
      if (!label) return
      setNotice(`Redid ${label.toLowerCase()}`)
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
    setNotice(`${targets.length} path${targets.length === 1 ? '' : 's'} copied`)
  }

  const menuItems = (): MenuItem[] => {
    const current = menu()
    if (!current) return []
    const targets = current.targets
    const one = targets.length === 1 ? targets[0] : null
    const many = (label: string) => (one ? label : `${label} (${targets.length})`)
    const allStarred = targets.every((r) => r.favorite)

    return [
      {
        id: 'play',
        label: 'Play',
        shortcut: 'Space',
        disabled: !one,
        onSelect: () => one && void play(one)
      },
      { id: 'd1', label: '', divider: true },
      {
        id: 'favorite',
        label: many(allStarred ? 'Remove from Favorites' : 'Add to Favorites'),
        onSelect: toggleFavoriteSelection
      },
      {
        id: 'category',
        label: many('Set Category…'),
        onSelect: () => setCategoryDialog(targets)
      },
      {
        id: 'clear-category',
        label: many('Clear Category'),
        disabled: targets.every((r) => r.category === 'Uncategorized' && !r.categoryManual),
        onSelect: () =>
          void editThenRefresh(
            targets.map((r) => r.path),
            () => api.clearCategory(targets.map((r) => r.path))
          )
      },
      { id: 'd2', label: '', divider: true },
      {
        id: 'reveal',
        label: 'Reveal in File Manager',
        disabled: !one,
        onSelect: () => one && reveal(one)
      },
      {
        id: 'show-folder',
        label: 'Show in Folders',
        disabled: !one,
        onSelect: () => {
          if (!one) return
          const dir = one.path.slice(0, Math.max(one.path.lastIndexOf('/'), one.path.lastIndexOf('\\')))
          setCategoryFilter(null, null)
          setFolderFilter(dir === activeLibrary()?.root ? null : dir)
        }
      },
      {
        id: 'copy-path',
        label: one ? 'Copy Path' : `Copy ${targets.length} Paths`,
        onSelect: () => copyPaths(targets)
      },
      { id: 'd3', label: '', divider: true },
      {
        id: 'drag',
        label: one ? 'Drag to DAW' : `Drag ${targets.length} to DAW`,
        disabled: true,
        // Dragging is done with the mouse; the menu entry exists so the feature is
        // discoverable, and it says how rather than pretending to be clickable.
        shortcut: 'drag a row'
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
    const inField =
      el?.tagName === 'INPUT' ||
      el?.tagName === 'TEXTAREA' ||
      el?.tagName === 'SELECT' ||
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

    if (event.code === 'Space' && !inField && !categoryDialog() && !settingsOpen() && !menu()) {
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
      if (menu()) setMenu(null)
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
        label: '★ Favorites',
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
        <div class="brand">ReSlip</div>

        <Show when={activeLibrary()}>
          {(library) => (
            <span class="lib-chip" title={library().root}>
              {library().name}
            </span>
          )}
        </Show>

        <div class="spacer" />

        <Show when={history().undo}>
          <button class="ghost" onClick={() => void undo()} title="Undo (Ctrl+Z)">
            ↶ Undo
          </button>
        </Show>
        <Show when={history().redo}>
          <button class="ghost" onClick={() => void redo()} title="Redo (Ctrl+Shift+Z)">
            ↷ Redo
          </button>
        </Show>

        <Show when={notice()}>
          <button class="notice" onClick={() => setNotice(null)} title="dismiss">
            {notice()}
          </button>
        </Show>

        <button
          class={settings().autoPlay ? 'ghost on' : 'ghost'}
          onClick={toggleAutoPlay}
          title="Play the row you click (Alt+A)"
        >
          Auto-play {settings().autoPlay ? 'on' : 'off'}
        </button>

        <button class="ghost" onClick={() => setSettingsOpen(true)} title="Settings (Ctrl+,)">
          ⚙ Settings
        </button>
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
          />
          <div class="rail-resize" onPointerDown={startResize} title="Drag to resize" />
        </div>

        <main class="main">
          <div class="main-head">
            <input
              class="search"
              placeholder="Search samples…"
              value={query()}
              onInput={(e) => {
                setQuery(e.currentTarget.value)
                scheduleSearch()
              }}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setQuery('')
                  void runSearch()
                  e.currentTarget.blur()
                }
              }}
            />
            {filterChips().map((chip) => (
              <button class="chip" onClick={chip.clear}>
                {chip.label} <span class="chip-x">×</span>
              </button>
            ))}
            <div class="spacer" />
            <Show when={matching() > 0}>
              <span class="dim small">
                {matching().toLocaleString()} match{matching() === 1 ? '' : 'es'}
              </span>
            </Show>
            <button class="ghost" onClick={() => void addLibrary()} title="Add a library (Ctrl+O)">
              Add library
            </button>
          </div>

          <Show
            when={libraries().length > 0}
            fallback={
              <div class="empty">
                <h1>{bootError() ? 'Backend error' : 'No library open'}</h1>
                <Show
                  when={bootError()}
                  fallback={
                    <p>
                      Point ReSlip at a folder of samples. The index is written into that folder, so
                      reopening it later costs one incremental pass.
                    </p>
                  }
                >
                  <p class="bad">{bootError()}</p>
                </Show>
                <button class="primary" onClick={() => void addLibrary()}>
                  Open library…
                </button>
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
            onStop={() => void stop()}
            onToggle={() => void togglePlayback()}
            onSeek={(seconds) => {
              const row = active()
              if (row) void play(row, seconds)
            }}
            onDecoded={setWaveMs}
            decodeMs={waveMs()}
          />
        </main>
      </div>

      <footer class="statusbar">
        <Show
          when={status()?.audioAvailable}
          fallback={<span class="bad">{status()?.audioError ?? 'audio device unavailable'}</span>}
        >
          <span class="good">●</span>
          <span>{status()?.deviceName}</span>
          <span class="dim">
            {status()?.sampleRate} Hz · {status()?.channels} ch · {status()?.sampleFormat}
          </span>
          <span class="dim">engine period {status()?.enginePeriodMs?.toFixed(2)} ms</span>
        </Show>

        <div class="spacer" />

        <Show when={selection().length > 1}>
          <span class="dim">{selection().length.toLocaleString()} selected</span>
        </Show>

        <Show when={libraries().length > 0}>
          <span class="dim">
            {rows().length.toLocaleString()} of {matching().toLocaleString()} loaded
          </span>
        </Show>

        <Show when={waveMs()}>
          {(ms) => <span class="dim">waveform {ms().toFixed(1)} ms</span>}
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
          onChange={changeSettings}
          libraries={libraries()}
          onClose={() => setSettingsOpen(false)}
          onReindex={(full) => void reindex(full)}
          onRemoveLibrary={(root) => void removeLibrary(root)}
          onAddLibrary={() => void addLibrary()}
          busy={indexing() !== null}
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
