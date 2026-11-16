import { For, Show, createMemo, createSignal } from 'solid-js'
import type { LibraryStatus } from './ipc'

/**
 * Settings, in a modal of its own.
 *
 * Scope is what a sample browser actually needs to configure, not a dumping ground:
 * playback behaviour, what the list shows, and the libraries themselves. Anything
 * that belongs to a file (category, favorite) stays on the row.
 *
 * Every setting here is persisted in `localStorage` under `reslip:*`. They are
 * per-machine display preferences, not library data, so they do not belong in the
 * SQLite index — copying a library folder to another machine should not carry
 * "this person prefers 200 rows a page".
 */
export interface Settings {
  autoPlay: boolean
  /** Rows fetched per page. Larger means fewer round trips, more DOM per window. */
  pageSize: number
  /** Infinite scroll: fetch the next page when the list nears its end. */
  infiniteScroll: boolean
  /** Show the category column in the table. */
  showCategory: boolean
}

export const DEFAULT_SETTINGS: Settings = {
  autoPlay: true,
  pageSize: 500,
  infiniteScroll: true,
  showCategory: true
}

const KEYS: Record<keyof Settings, string> = {
  autoPlay: 'reslip:autoplay',
  pageSize: 'reslip:page-size',
  infiniteScroll: 'reslip:infinite-scroll',
  showCategory: 'reslip:show-category'
}

export function loadSettings(): Settings {
  const read = (key: keyof Settings) => localStorage.getItem(KEYS[key])
  const pageSize = Number(read('pageSize'))
  return {
    autoPlay: read('autoPlay') !== 'false',
    pageSize: Number.isFinite(pageSize) && pageSize >= 100 ? pageSize : DEFAULT_SETTINGS.pageSize,
    infiniteScroll: read('infiniteScroll') !== 'false',
    showCategory: read('showCategory') !== 'false'
  }
}

export function saveSettings(settings: Settings): void {
  localStorage.setItem(KEYS.autoPlay, String(settings.autoPlay))
  localStorage.setItem(KEYS.pageSize, String(settings.pageSize))
  localStorage.setItem(KEYS.infiniteScroll, String(settings.infiniteScroll))
  localStorage.setItem(KEYS.showCategory, String(settings.showCategory))
}

interface Props {
  settings: Settings
  onChange: (settings: Settings) => void
  libraries: LibraryStatus[]
  onClose: () => void
  onReindex: (full: boolean) => void
  onRemoveLibrary: (root: string) => void
  onAddLibrary: () => void
  busy: boolean
}

export function SettingsDialog(props: Props) {
  const [draft, setDraft] = createSignal<Settings>({ ...props.settings })

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    const next = { ...draft(), [key]: value }
    setDraft(next)
    // Applied immediately: a settings dialog with an OK button that can be
    // forgotten is a dialog people close by accident.
    props.onChange(next)
  }

  const activeLibrary = createMemo(() => props.libraries.find((l) => l.active) ?? null)

  return (
    <div class="modal-backdrop" onClick={props.onClose}>
      <div class="modal settings" onClick={(e) => e.stopPropagation()} role="dialog">
        <h2 class="modal-title">Settings</h2>

        <section class="settings-section">
          <h3>Playback</h3>
          <label class="setting">
            <input
              type="checkbox"
              checked={draft().autoPlay}
              onChange={(e) => set('autoPlay', e.currentTarget.checked)}
            />
            <span>
              Play the row I click
              <span class="dim"> · Alt+A toggles it without opening this dialog</span>
            </span>
          </label>
        </section>

        <section class="settings-section">
          <h3>List</h3>
          <label class="setting">
            <input
              type="checkbox"
              checked={draft().infiniteScroll}
              onChange={(e) => set('infiniteScroll', e.currentTarget.checked)}
            />
            <span>Load more rows as I scroll</span>
          </label>
          <label class="setting">
            <span class="setting-label">Rows per page</span>
            <select
              value={String(draft().pageSize)}
              onChange={(e) => set('pageSize', Number(e.currentTarget.value))}
            >
              <For each={[200, 500, 1000, 2000]}>
                {(n) => <option value={String(n)}>{n.toLocaleString()}</option>}
              </For>
            </select>
          </label>
          <label class="setting">
            <input
              type="checkbox"
              checked={draft().showCategory}
              onChange={(e) => set('showCategory', e.currentTarget.checked)}
            />
            <span>Show the category column</span>
          </label>
        </section>

        <section class="settings-section">
          <h3>Libraries</h3>
          <Show
            when={props.libraries.length > 0}
            fallback={<p class="dim">No library open.</p>}
          >
            <ul class="library-list">
              <For each={props.libraries}>
                {(library) => (
                  <li class="library-row">
                    <div class="library-row-main">
                      <span class="library-row-name" title={library.root}>
                        {library.name}
                      </span>
                      <span class="count">{library.count.toLocaleString()}</span>
                    </div>
                    <span class="library-row-path dim" title={library.root}>
                      {library.root}
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <div class="settings-actions">
            <button class="ghost" onClick={props.onAddLibrary} disabled={props.busy}>
              Add library…
            </button>
            <button
              class="ghost"
              onClick={() => props.onReindex(false)}
              disabled={props.busy || !activeLibrary()}
              title="Rescan for new and changed files"
            >
              Reindex
            </button>
            <button
              class="ghost"
              onClick={() => props.onReindex(true)}
              disabled={props.busy || !activeLibrary()}
              title="Re-probe every file and drop vanished ones"
            >
              Full reindex
            </button>
            <Show when={activeLibrary()}>
              {(library) => (
                <button
                  class="ghost danger"
                  onClick={() => props.onRemoveLibrary(library().root)}
                  disabled={props.busy}
                >
                  Close library
                </button>
              )}
            </Show>
          </div>
          <p class="modal-note dim">
            A reindex blocks the window until it finishes, because a half-indexed
            library shows a list that is missing rows you cannot see are missing.
          </p>
        </section>

        <div class="modal-actions">
          <div class="spacer" />
          <button class="primary" onClick={props.onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  )
}
