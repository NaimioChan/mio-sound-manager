import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import {
  api,
  type AppStatus,
  type AudioDevice,
  type LibraryStatus
} from './ipc'
import { THEMES, findTheme, type Theme } from './theme'

/**
 * Settings: device, library, appearance.
 *
 * Three groups in one dialog rather than a settings tree, because there are
 * eleven settings in total and a tree would be more navigation than content. The
 * library group edits the *active* library; per-library management (reindex,
 * close) is on the rail's right-click menu, which is where it can name which
 * library it applies to.
 */
export interface Settings {
  /** Audition a row as soon as it is clicked. */
  autoPlay: boolean
  /** Ask the index for the next page when the list nears its end. */
  infiniteScroll: boolean
  /** Show the category column; the value is still filterable from the rail. */
  showCategory: boolean
  /** Rows per search request. */
  pageSize: number
}

const STORAGE_KEY = 'mio:settings'

const DEFAULTS: Settings = {
  autoPlay: true,
  infiniteScroll: true,
  showCategory: true,
  pageSize: 500
}

/**
 * Read the settings, one key at a time.
 *
 * Not one JSON blob: an older build that wrote `{autoPlay, pageSize}` must not
 * make a newer one fall back to every default because `theme` is missing. Per-key
 * reads also mean a corrupt value costs one setting instead of all of them.
 */
export function loadSettings(): Settings {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (!raw) return { ...DEFAULTS }
  try {
    const parsed = JSON.parse(raw) as Partial<Settings>
    return {
      autoPlay: typeof parsed.autoPlay === 'boolean' ? parsed.autoPlay : DEFAULTS.autoPlay,
      infiniteScroll:
        typeof parsed.infiniteScroll === 'boolean' ? parsed.infiniteScroll : DEFAULTS.infiniteScroll,
      showCategory:
        typeof parsed.showCategory === 'boolean' ? parsed.showCategory : DEFAULTS.showCategory,
      pageSize:
        typeof parsed.pageSize === 'number' && parsed.pageSize > 0
          ? Math.min(5000, parsed.pageSize)
          : DEFAULTS.pageSize
    }
  } catch {
    return { ...DEFAULTS }
  }
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings))
  } catch {
    // A locked profile costs the settings, not the session.
  }
}

interface Props {
  settings: Settings
  /** The applied theme's id. Owned by `theme.ts`, not by the settings blob. */
  theme: string
  onChange: (patch: Partial<Settings>) => void
  /** Theme choice goes through its own callback: `theme.ts` owns that state. */
  onTheme: (id: string) => void
  status: AppStatus | null
  libraries: LibraryStatus[]
  activeLibrary: LibraryStatus | null
  onStatus: (next: AppStatus) => void
  onAddLibrary: () => void
  onClose: () => void
}

/**
 * The theme picker's swatch.
 *
 * Each card paints its own palette rather than the applied one, which is the whole
 * point of a swatch: a light theme has to be recognisable while a dark one is in
 * effect. The three colours shown are the ones a theme is *made of* — background,
 * accent and the neutral its text is derived from.
 */
function ThemeCard(props: { theme: Theme; current: boolean; onPick: () => void }) {
  const swatch = () => {
    const p = props.theme.palette
    if (!p) {
      // iX's own palettes: the two colours that identify them.
      return props.theme.schema === 'dark'
        ? ['#0f1619', '#283236', '#00bde3', '#00eaff']
        : ['#ffffff', '#eff0f1', '#006e93', '#199fff']
    }
    return [p.bg, p.accent, p.dynamic, p.fg]
  }

  return (
    <button
      class={props.current ? 'theme-card on' : 'theme-card'}
      onClick={props.onPick}
      title={props.theme.hint}
      data-theme-id={props.theme.id}
    >
      <span class="theme-swatch">
        <For each={swatch()}>{(color) => <span style={{ background: color }} />}</For>
      </span>
      <span class="theme-card-name">{props.theme.label}</span>
      <span class="theme-card-hint">{props.theme.hint}</span>
    </button>
  )
}

export function SettingsDialog(props: Props) {
  const [devices, setDevices] = createSignal<AudioDevice[]>([])
  const [asioCompiled, setAsioCompiled] = createSignal(false)
  const [deviceError, setDeviceError] = createSignal<string | null>(null)
  let modal:
    | (HTMLElement & { showModal: () => Promise<void>; closeModal: (r?: unknown) => Promise<void> })
    | undefined

  const open = (el: HTMLElement) => {
    modal = el as typeof modal
    queueMicrotask(() => void modal?.showModal())
  }

  const close = () => {
    void modal?.closeModal()
    props.onClose()
  }

  // The device list is read once when the dialog opens: enumerating ASIO drivers
  // loads each driver in an STA thread and takes a visible moment, so it must not
  // be on a render path that can run twice.
  createEffect(() => {
    void (async () => {
      try {
        setAsioCompiled(await api.asioCompiled())
        setDevices(await api.audioDevices())
      } catch (e) {
        setDeviceError(String(e))
      }
    })()
  })

  const currentDevice = createMemo(() => {
    const status = props.status
    if (!status) return ''
    return `${status.backend ?? ''}|${status.deviceName ?? ''}`
  })

  const switchDevice = async (backend: string, device: string) => {
    setDeviceError(null)
    try {
      const next = await api.setAudioDevice(backend, device === '' ? null : device, null)
      props.onStatus(next)
    } catch (e) {
      setDeviceError(String(e))
    }
  }

  return (
    <ix-modal ref={open} size="720">
      <ix-modal-header heading="Settings" subHeading="Device, library, appearance" />
      <ix-modal-content>
        <div class="modal-form">
          {/* ---------------------------------------------------------- audio */}
          <section class="settings-section">
            <h3>Output device</h3>

            <Show
              when={props.status?.audioAvailable}
              fallback={
                <div class="setting-hint" style={{ color: 'var(--theme-color-alarm-text)' }}>
                  {props.status?.audioError ?? 'no audio device open'}
                </div>
              }
            >
              <div class="setting-hint">
                {props.status?.backend} · {props.status?.deviceName} · {props.status?.sampleRate} Hz ·{' '}
                {props.status?.channels} ch · {props.status?.sampleFormat} · engine period{' '}
                {props.status?.enginePeriodMs?.toFixed(2)} ms
              </div>
            </Show>

            <div class="setting">
              <span class="setting-label">Device</span>
              <div style={{ width: '320px' }}>
                <ix-select
                  value={currentDevice()}
                  allowClear={false}
                  on:valueChange={(e: CustomEvent<string | string[]>) => {
                    const [backend, ...rest] = String(e.detail).split('|')
                    void switchDevice(backend ?? 'wasapi', rest.join('|'))
                  }}
                >
                  <For each={devices()}>
                    {(device) => (
                      <ix-select-item
                        value={`${device.backend}|${device.name}`}
                        label={`${device.backend.toUpperCase()} · ${device.name}`}
                      />
                    )}
                  </For>
                </ix-select>
              </div>
            </div>

            <Show when={!asioCompiled()}>
              <div class="setting-hint">
                ASIO is not compiled into this build. Rebuild with
                <code> ./tools/build-app.sh --asio</code> to offer ASIO drivers.
              </div>
            </Show>
            <Show when={deviceError()}>
              {(err) => (
                <div class="setting-hint" style={{ color: 'var(--theme-color-alarm-text)' }}>
                  {err()}
                </div>
              )}
            </Show>
          </section>

          {/* -------------------------------------------------------- library */}
          <section class="settings-section">
            <h3>Library</h3>

            <Show
              when={props.activeLibrary}
              fallback={<div class="setting-hint">No library is open.</div>}
            >
              {(library) => (
                <>
                  <div class="setting">
                    <span class="setting-label">{library().name}</span>
                    <span class="setting-hint">
                      {library().count.toLocaleString()} sounds · {library().root}
                    </span>
                  </div>
                  <div class="setting">
                    <span class="setting-label">Reveal the index</span>
                    <ix-button
                      variant="secondary"
                      onClick={() => void api.revealInFolder(library().root)}
                    >
                      Open folder
                    </ix-button>
                  </div>
                </>
              )}
            </Show>

            <div class="setting">
              <span class="setting-label">Add another library</span>
              <ix-button variant="secondary" onClick={props.onAddLibrary}>
                Open library…
              </ix-button>
            </div>

            <div class="setting">
              <span class="setting-label">
                Rows per page
                <div class="setting-hint">
                  How many rows one search returns. Larger pages scroll further between
                  fetches.
                </div>
              </span>
              <div style={{ width: '140px' }}>
                <ix-select
                  value={String(props.settings.pageSize)}
                  on:valueChange={(e: CustomEvent<string | string[]>) =>
                    props.onChange({ pageSize: Number(e.detail) })
                  }
                >
                  <ix-select-item value="200" label="200" />
                  <ix-select-item value="500" label="500" />
                  <ix-select-item value="1000" label="1000" />
                  <ix-select-item value="2000" label="2000" />
                </ix-select>
              </div>
            </div>

            <div class="setting">
              <span class="setting-label">
                Show the category column
                <div class="setting-hint">
                  Turn it off for a denser list. Filtering by category still works from the
                  rail.
                </div>
              </span>
              <ix-toggle
                checked={props.settings.showCategory}
                on:checkedChange={(e: CustomEvent<boolean>) =>
                  props.onChange({ showCategory: e.detail })
                }
              />
            </div>
          </section>

          {/* ------------------------------------------------------ behaviour */}
          <section class="settings-section">
            <h3>Behaviour</h3>

            <div class="setting">
              <span class="setting-label">
                Auto-play on select
                <div class="setting-hint">
                  Clicking a row auditions it. The same switch as AUTO PLAY in the dock.
                </div>
              </span>
              <ix-toggle
                checked={props.settings.autoPlay}
                on:checkedChange={(e: CustomEvent<boolean>) => props.onChange({ autoPlay: e.detail })}
              />
            </div>

            <div class="setting">
              <span class="setting-label">
                Infinite scroll
                <div class="setting-hint">
                  Fetch the next page as the list nears its end, instead of showing one page.
                </div>
              </span>
              <ix-toggle
                checked={props.settings.infiniteScroll}
                on:checkedChange={(e: CustomEvent<boolean>) =>
                  props.onChange({ infiniteScroll: e.detail })
                }
              />
            </div>
          </section>

          {/* ------------------------------------------------------ appearance */}
          <section class="settings-section">
            <h3>Appearance</h3>
            <div class="setting-hint">
              Themes repaint the Siemens iX token set, so every control, panel and the
              waveform follow the same palette.
            </div>
            <div class="theme-grid">
              <For each={THEMES}>
                {(theme) => (
                  <ThemeCard
                    theme={theme}
                    current={props.theme === theme.id}
                    onPick={() => props.onTheme(theme.id)}
                  />
                )}
              </For>
            </div>
            <div class="setting-hint">
              Current: {findTheme(props.theme).label}
            </div>
          </section>
        </div>
      </ix-modal-content>
      <ix-modal-footer>
        <div class="modal-actions">
          <div class="spacer" />
          <ix-button variant="primary" onClick={close}>
            Done
          </ix-button>
        </div>
      </ix-modal-footer>
    </ix-modal>
  )
}
