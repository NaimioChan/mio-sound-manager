import { Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import {
  api,
  type AppStatus,
  type Facet,
  type FolderNode,
  type SoundRow
} from './ipc'
import { SoundTable, DEFAULT_DIR, type RowMenuRequest, type SortKey } from './SoundTable'
import { WaveformDock } from './WaveformDock'
import { Sidebar } from './Sidebar'
import { ContextMenu, type MenuItem } from './ContextMenu'
import { CategoryDialog } from './CategoryDialog'

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

/** How many rows one page of the list asks for. The index answers in ~2ms. */
const PAGE = 500

export function App() {
  const [status, setStatus] = createSignal<AppStatus | null>(null)
  const [bootError, setBootError] = createSignal<string | null>(null)
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

  const [autoPlay, setAutoPlay] = createSignal(
    localStorage.getItem('reslip:autoplay') !== 'false'
  )
  const [menu, setMenu] = createSignal<RowMenuRequest | null>(null)
  const [categoryDialog, setCategoryDialog] = createSignal<SoundRow[] | null>(null)

  const [notice, setNotice] = createSignal<string | null>(null)
  const [waveMs, setWaveMs] = createSignal<number | null>(null)
  const [busy, setBusy] = createSignal(false)

  const playhead = usePlayhead()

  /**
   * The search request, derived once so the search call and Ctrl+A cannot drift
   * apart: selecting "all" must select exactly what the list is showing.
   */
  const request = createMemo(() => ({
    query: query(),
    category: category() ?? undefined,
    subcategory: subcategory() ?? undefined,
    folder: folder() ?? undefined,
    favoriteOnly: favoritesOnly() ? true : undefined,
    sortBy: sortKey(),
    sortDir: (sortAsc() ? 'asc' : 'desc') as 'asc' | 'desc',
    limit: PAGE
  }))

  const runSearch = async () => {
    if (!status()?.library) {
      setRows([])
      setMatching(0)
      return
    }
    const req = request()
    try {
      // The page and the total are asked for together: the status bar reads "38
      // of 27,525", and a total fetched separately would sometimes belong to the
      // previous query.
      const [page, count] = await Promise.all([api.search(req), api.countMatching(req)])
      setRows(page)
      setMatching(count)
    } catch (e) {
      setNotice(String(e))
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

  const refresh = async () => {
    try {
      const next = await api.status()
      setStatus(next)
      setBootError(null)
      if (next.library) {
        await Promise.all([loadTree(), loadSidebar()])
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

  const openLibrary = async () => {
    setBusy(true)
    try {
      const picked = await api.pickLibraryFolder()
      if (!picked) return
      const lib = await api.openLibrary(picked)
      // A 280k-file pass takes a minute; the count is only meaningful once it
      // ends, so the notice says where the library is and the status bar carries
      // the progress.
      setNotice(`opened ${lib.root.split(/[\\/]/).pop()}`)
      await refresh()
    } catch (e) {
      setNotice(String(e))
    } finally {
      setBusy(false)
    }
  }

  const changeSelection = (paths: string[], next: SoundRow | null) => {
    setSelection(paths)
    if (next) setActive(next)
  }

  const activate = (row: SoundRow) => {
    if (autoPlay()) void play(row)
    else setActive(row)
  }

  const toggleAutoPlay = () => {
    const next = !autoPlay()
    setAutoPlay(next)
    localStorage.setItem('reslip:autoplay', String(next))
  }

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
      const paths = await api.matchingPaths(request())
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

  /** Apply a library edit, then refresh the counts and the visible rows. */
  const editThenRefresh = async (paths: string[], apply: () => Promise<unknown>) => {
    if (paths.length === 0) return
    try {
      await apply()
      await Promise.all([loadSidebar(), runSearch()])
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
          setFolderFilter(dir === status()?.library?.root ? null : dir)
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
   * Shortcuts that belong to the app rather than the table: Space, Ctrl+O, and
   * the auto-play toggle. The table owns arrow keys and Ctrl+A.
   */
  const onKeyDown = (event: KeyboardEvent) => {
    const el = document.activeElement as HTMLElement | null
    const inField =
      el?.tagName === 'INPUT' || el?.tagName === 'TEXTAREA' || el?.isContentEditable === true

    if (event.code === 'Space' && !inField && !categoryDialog() && !menu()) {
      event.preventDefault()
      void togglePlayback()
      return
    }

    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'o') {
      event.preventDefault()
      void openLibrary()
      return
    }

    if (event.altKey && event.key.toLowerCase() === 'a' && !inField) {
      event.preventDefault()
      toggleAutoPlay()
      return
    }

    if (event.key === 'Escape' && menu()) {
      setMenu(null)
    }
  }

  onMount(() => {
    void refresh()
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  // While a pass runs, poll the library so the sidebar count grows instead of
  // sitting at zero until the whole pass ends.
  onMount(() => {
    const timer = window.setInterval(async () => {
      const current = status()
      if (!current?.library?.indexing) return
      try {
        const lib = await api.library()
        if (!lib) return
        setStatus({ ...current, library: lib })
        if (!lib.indexing) {
          await Promise.all([loadTree(), loadSidebar()])
          await runSearch()
        }
      } catch {
        // best effort
      }
    }, 500)
    onCleanup(() => clearInterval(timer))
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
      chips.push({ label: '★ Favorites', clear: () => { setFavoritesOnly(false); void runSearch() } })
    }
    return chips
  }

  return (
    <div class="app">
      <header class="topbar">
        <div class="brand">ReSlip</div>

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

        <Show when={notice()}>
          <button class="notice" onClick={() => setNotice(null)} title="dismiss">
            {notice()}
          </button>
        </Show>

        <button
          class={autoPlay() ? 'ghost on' : 'ghost'}
          onClick={toggleAutoPlay}
          title="Play the row you click (Alt+A)"
        >
          Auto-play {autoPlay() ? 'on' : 'off'}
        </button>

        <button class="ghost" onClick={() => void openLibrary()} disabled={busy()}>
          {status()?.library ? 'Change library' : 'Open library'}
        </button>
      </header>

      <div class="body">
        <Sidebar
          folders={folders()}
          library={status()?.library ?? null}
          facets={facets()}
          favorites={favorites()}
          total={total()}
          category={category()}
          subcategory={subcategory()}
          folder={folder()}
          favoritesOnly={favoritesOnly()}
          onCategory={setCategoryFilter}
          onFolder={setFolderFilter}
          onToggleFavorites={() => {
            setFavoritesOnly(!favoritesOnly())
            void runSearch()
          }}
        />

        <main class="main">
          <Show
            when={status()?.library}
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
                <button class="primary" onClick={() => void openLibrary()} disabled={busy()}>
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
              onSort={changeSort}
              onSelectionChange={changeSelection}
              onActivate={activate}
              onRowMenu={setMenu}
              onToggleFavorite={toggleFavorite}
              onSelectAll={() => void selectAll()}
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

        <Show when={status()?.library}>
          {(lib) => (
            <Show
              when={!lib().indexing}
              fallback={
                <span class="dim">
                  indexing {lib().indexed.toLocaleString()} / {lib().total.toLocaleString()}
                </span>
              }
            >
              <span class="dim">
                {rows().length.toLocaleString()} of {matching().toLocaleString()} sounds
              </span>
            </Show>
          )}
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
    </div>
  )
}
