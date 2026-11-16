import { Show, createSignal, onCleanup, onMount } from 'solid-js'
import { api, type AppStatus, type FolderNode, type SoundRow } from './ipc'
import { SoundTable } from './SoundTable'
import { WaveformDock } from './WaveformDock'
import { Sidebar } from './Sidebar'

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

  return { positionS, durationS, playing }
}

export function App() {
  const [status, setStatus] = createSignal<AppStatus | null>(null)
  const [bootError, setBootError] = createSignal<string | null>(null)
  const [rows, setRows] = createSignal<SoundRow[]>([])
  const [selected, setSelected] = createSignal<SoundRow | null>(null)
  const [folders, setFolders] = createSignal<FolderNode | null>(null)

  const [query, setQuery] = createSignal('')
  const [category, setCategory] = createSignal<string | null>(null)
  const [folder, setFolder] = createSignal<string | null>(null)

  const [notice, setNotice] = createSignal<string | null>(null)
  const [waveMs, setWaveMs] = createSignal<number | null>(null)
  const [busy, setBusy] = createSignal(false)

  const playhead = usePlayhead()

  const runSearch = async () => {
    if (!status()?.library) {
      setRows([])
      return
    }
    try {
      setRows(
        await api.search({
          query: query(),
          category: category() ?? undefined,
          folder: folder() ?? undefined,
          sortBy: 'filename',
          sortDir: 'asc',
          limit: 500
        })
      )
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

  const refresh = async () => {
    try {
      const next = await api.status()
      setStatus(next)
      setBootError(null)
      if (next.library) await loadTree()
      await runSearch()
    } catch (e) {
      setBootError(String(e))
    }
  }

  const play = async (row: SoundRow, startAt?: number) => {
    setSelected(row)
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

  const openLibrary = async () => {
    const root = window.prompt('Library folder (absolute path)', status()?.library?.root ?? '')
    if (!root) return
    setBusy(true)
    try {
      const lib = await api.openLibrary(root)
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

  onMount(() => void refresh())

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
          await loadTree()
          await runSearch()
        }
      } catch {
        // best effort
      }
    }, 500)
    onCleanup(() => clearInterval(timer))
  })

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
        />

        <Show when={category()}>
          <button
            class="chip"
            onClick={() => {
              setCategory(null)
              void runSearch()
            }}
          >
            {category()} <span class="chip-x">×</span>
          </button>
        </Show>

        <Show when={folder()}>
          <button
            class="chip"
            onClick={() => {
              setFolder(null)
              void runSearch()
            }}
          >
            {folder()?.split(/[\\/]/).pop()} <span class="chip-x">×</span>
          </button>
        </Show>

        <div class="spacer" />

        <Show when={notice()}>
          <button class="notice" onClick={() => setNotice(null)} title="dismiss">
            {notice()}
          </button>
        </Show>

        <button class="ghost" onClick={openLibrary} disabled={busy()}>
          {status()?.library ? 'Change library' : 'Open library'}
        </button>
      </header>

      <div class="body">
        <Sidebar
          folders={folders()}
          library={status()?.library ?? null}
          selectedCategory={category()}
          selectedFolder={folder()}
          onCategory={(value) => {
            setCategory(value)
            void runSearch()
          }}
          onFolder={(value) => {
            setFolder(value)
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
                <button class="primary" onClick={openLibrary} disabled={busy()}>
                  Open library…
                </button>
              </div>
            }
          >
            <SoundTable rows={rows()} selected={selected()} onSelect={(row) => void play(row)} />
          </Show>

          <WaveformDock
            row={selected()}
            positionS={playhead.positionS()}
            durationS={playhead.durationS()}
            playing={playhead.playing()}
            onStop={() => void stop()}
            onSeek={(seconds) => {
              const row = selected()
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
              <span class="dim">{lib().count.toLocaleString()} sounds</span>
            </Show>
          )}
        </Show>

        <Show when={waveMs()}>
          {(ms) => <span class="dim">waveform {ms().toFixed(1)} ms</span>}
        </Show>
      </footer>
    </div>
  )
}
