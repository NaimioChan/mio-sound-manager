import { For, Show, createMemo, createSignal, onMount } from 'solid-js'
import { api, type AppStatus, type AudioDevice } from './ipc'

/**
 * Settings, in a modal of its own.
 *
 * Scope is what a sample browser actually needs to configure, not a dumping ground:
 * the output device, playback behaviour, and what the list shows. Anything that
 * belongs to a file (category, favorite) stays on the row, and anything that
 * belongs to one library (reindex, close) lives in that library's context menu in
 * the rail — a modal is the wrong place to manage several of them.
 *
 * Display settings are persisted in `localStorage` under `reslip:*`. They are
 * per-machine preferences, not library data, so they do not belong in the SQLite
 * index — copying a library folder to another machine should not carry "this
 * person prefers 200 rows a page". The audio device is different: it is a
 * property of the machine's hardware, so the backend writes it into
 * `%APPDATA%\com.naimio.reslip\audio.json` next to the session.
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

/**
 * Buffer sizes offered for ASIO.
 *
 * A driver rejects a size outside its own min/max range, and a free-form field
 * would mostly fail. These are the sizes every interface supports; the driver's
 * own preferred size is added to the list when it is something else.
 */
const ASIO_BUFFERS = [64, 128, 256, 512, 1024, 2048]

interface Props {
  settings: Settings
  onChange: (settings: Settings) => void
  status: AppStatus | null
  onStatus: (status: AppStatus) => void
  onClose: () => void
}

export function SettingsDialog(props: Props) {
  const [draft, setDraft] = createSignal<Settings>({ ...props.settings })
  const [devices, setDevices] = createSignal<AudioDevice[]>([])
  const [asioCompiled, setAsioCompiled] = createSignal(true)
  const [switching, setSwitching] = createSignal(false)
  const [deviceError, setDeviceError] = createSignal<string | null>(null)

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    const next = { ...draft(), [key]: value }
    setDraft(next)
    // Applied immediately: a settings dialog with an OK button that can be
    // forgotten is a dialog people close by accident.
    props.onChange(next)
  }

  onMount(() => {
    api
      .audioDevices()
      .then(setDevices)
      .catch((e) => setDeviceError(String(e)))
    api.asioCompiled().then(setAsioCompiled).catch(() => setAsioCompiled(false))
  })

  const wasapi = createMemo(() => devices().filter((d) => d.backend === 'WASAPI'))
  const asio = createMemo(() => devices().filter((d) => d.backend === 'ASIO'))

  /** The backend the running engine is on, not what the picker shows. */
  const activeBackend = createMemo(() => props.status?.backend ?? 'WASAPI')

  const apply = async (backend: string, device: string | null, bufferFrames: number | null) => {
    setSwitching(true)
    setDeviceError(null)
    try {
      const status = await api.setAudioDevice(backend, device, bufferFrames)
      props.onStatus(status)
      if (!status.audioAvailable) setDeviceError(status.audioError ?? 'device did not open')
    } catch (e) {
      setDeviceError(String(e))
    } finally {
      setSwitching(false)
    }
  }

  return (
    <div class="modal-backdrop" onClick={props.onClose}>
      <div class="modal settings" onClick={(e) => e.stopPropagation()} role="dialog">
        <h2 class="modal-title">Settings</h2>

        <section class="settings-section">
          <h3>Audio</h3>

          <label class="setting">
            <span class="setting-label">Output device</span>
            <select
              value={`${activeBackend()}:${props.status?.deviceName ?? ''}`}
              disabled={switching()}
              onChange={(e) => {
                // A driver name can contain a colon, so only the first one splits.
                const value = e.currentTarget.value
                const at = value.indexOf(':')
                const backend = value.slice(0, at)
                const device = value.slice(at + 1)
                void apply(backend, device || null, null)
              }}
            >
              <Show when={wasapi().length > 0}>
                <optgroup label="WASAPI (shared)">
                  <For each={wasapi()}>
                    {(device) => (
                      <option value={`WASAPI:${device.name}`}>
                        {device.label}
                        {device.default ? ' — default' : ''}
                      </option>
                    )}
                  </For>
                </optgroup>
              </Show>
              <Show when={asio().length > 0}>
                <optgroup label="ASIO">
                  <For each={asio()}>
                    {(device) => (
                      <option value={`ASIO:${device.name}`} disabled={!device.available}>
                        {device.label}
                        {device.channels ? ` — ${device.channels} ch` : ''}
                        {device.available ? '' : ' — unavailable'}
                      </option>
                    )}
                  </For>
                </optgroup>
              </Show>
            </select>
          </label>

          <Show when={activeBackend() === 'ASIO'}>
            <label class="setting">
              <span class="setting-label">Buffer size</span>
              <select
                value={String(props.status?.endpointFrames ?? '')}
                disabled={switching()}
                onChange={(e) =>
                  void apply(
                    'ASIO',
                    props.status?.deviceName ?? null,
                    Number(e.currentTarget.value)
                  )
                }
              >
                {/* The driver's preferred size is not necessarily one of the
                    standard ones, so it is shown when it is the running value. */}
                <Show
                  when={ASIO_BUFFERS.includes(props.status?.endpointFrames ?? 0)}
                  fallback={
                    <option value={String(props.status?.endpointFrames ?? '')}>
                      {props.status?.endpointFrames} frames
                    </option>
                  }
                >
                  <For each={ASIO_BUFFERS}>
                    {(frames) => <option value={String(frames)}>{frames} frames</option>}
                  </For>
                </Show>
              </select>
            </label>
          </Show>

          <Show when={!asioCompiled()}>
            <p class="setting-note dim">This build has no ASIO support.</p>
          </Show>
          <Show when={asioCompiled() && asio().length === 0}>
            <p class="setting-note dim">No ASIO driver installed.</p>
          </Show>

          <Show when={deviceError()}>
            {(err) => <p class="setting-note bad">{err()}</p>}
          </Show>

          <Show when={props.status?.audioAvailable}>
            <p class="setting-note dim">
              {props.status?.backend} · {props.status?.sampleRate} Hz ·{' '}
              {props.status?.channels} ch · {props.status?.sampleFormat} ·{' '}
              {props.status?.enginePeriodMs?.toFixed(2)} ms period
            </p>
          </Show>
        </section>

        <section class="settings-section">
          <h3>Playback</h3>
          <label class="setting">
            <input
              type="checkbox"
              checked={draft().autoPlay}
              onChange={(e) => set('autoPlay', e.currentTarget.checked)}
            />
            <span>Play on click</span>
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
