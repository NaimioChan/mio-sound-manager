import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import {
  api,
  type AppStatus,
  type AudioDevice,
  type LibraryStatus
} from './ipc'
import { LOCALES, locale, setLocale, t, type Locale } from './i18n'
import { THEMES, type Theme } from './theme'
import { ZOOM_STEPS } from './zoom'

/**
 * Settings: device, library, behaviour, appearance.
 *
 * Four groups in one dialog rather than a settings tree, because there are a
 * dozen settings in total and a tree would be more navigation than content. The
 * library group edits the *active* library; per-library management (reindex,
 * close) is on the rail's right-click menu, which is where it can name which
 * library it applies to.
 *
 * Nothing here explains itself in a second line. A row is a label and its
 * control; the only text allowed under a label is a state the label cannot
 * carry — the device error, the format the engine actually opened, or the count
 * of what an action will apply to.
 */
export interface Settings {
  /** Audition a row as soon as it is clicked. */
  autoPlay: boolean
  /** Show the category column; the value is still filterable from the rail. */
  showCategory: boolean
}

const STORAGE_KEY = 'mio:settings'

const DEFAULTS: Settings = {
  autoPlay: true,
  showCategory: true
}

/**
 * Read the settings, one key at a time.
 *
 * Not one JSON blob: an older build may have extra paging keys. Per-key
 * reads also mean a corrupt value costs one setting instead of all of them.
 */
export function loadSettings(): Settings {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (!raw) return { ...DEFAULTS }
  try {
    const parsed = JSON.parse(raw) as Partial<Settings>
    return {
      autoPlay: typeof parsed.autoPlay === 'boolean' ? parsed.autoPlay : DEFAULTS.autoPlay,
      showCategory:
        typeof parsed.showCategory === 'boolean' ? parsed.showCategory : DEFAULTS.showCategory
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
  /** The applied interface scale, as a percentage. */
  zoom: number
  onChange: (patch: Partial<Settings>) => void
  /** Theme choice goes through its own callback: `theme.ts` owns that state. */
  onTheme: (id: string) => void
  /** Same for the zoom: `zoom.ts` owns the webview, this is only the picker. */
  onZoom: (percent: number) => void
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
 * effect. The four colours shown are the ones a theme is *made of* — background,
 * the two accents and the neutral its text is derived from.
 */
function ThemeCard(props: { theme: Theme; current: boolean; onPick: () => void }) {
  const swatch = () => {
    const p = props.theme.palette
    return [p.bg, p.accent, p.dynamic, p.fg]
  }

  return (
    <button
      class={props.current ? 'theme-card on' : 'theme-card'}
      onClick={props.onPick}
      title={t(props.theme.labelKey)}
      data-theme-id={props.theme.id}
    >
      <span class="theme-swatch">
        <For each={swatch()}>{(color) => <span style={{ background: color }} />}</For>
      </span>
      <span class="theme-card-name">{t(props.theme.labelKey)}</span>
    </button>
  )
}

export function SettingsDialog(props: Props) {
  const [devices, setDevices] = createSignal<AudioDevice[]>([])
  const [asioCompiled, setAsioCompiled] = createSignal(true)
  const [deviceError, setDeviceError] = createSignal<string | null>(null)
  let modal:
    | (HTMLElement & { showModal: () => Promise<void>; closeModal: (r?: unknown) => Promise<void> })
    | undefined

  const open = (el: HTMLElement) => {
    modal = el as typeof modal
    queueMicrotask(() => void modal?.showModal())
  }

  /**
   * Close from inside the dialog (the Done button).
   *
   * The signal has to drop on the same tick as the request: iX's close is
   * animated, and a parent that unmounts the element mid-animation leaves the
   * next mount with no `showModal` to run on. `props.onClose()` unmounts, so it
   * cannot wait for `closeModal` to resolve.
   */
  const close = () => {
    void modal?.closeModal()
    props.onClose()
  }

  /**
   * iX closes itself when the header's × or Escape is used, without going
   * through `close`.
   *
   * Before this, the × left `settingsOpen` true while the element was already
   * dismissed: the app believed a dialog was up, so the next click on the gear
   * flipped the same flag to the same value and nothing was rendered. The window
   * had to be restarted. Both of iX's events land here, so the state follows
   * whatever actually closed it.
   */
  const onDialogClosed = () => {
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
    <ix-modal
      ref={open}
      size="720"
      on:dialogClose={onDialogClosed}
      on:dialogDismiss={onDialogClosed}
    >
      <ix-modal-header heading={t('settings.heading')} />
      <ix-modal-content>
        <div class="modal-form">
          {/* ---------------------------------------------------------- audio */}
          <section class="settings-section">
            <h3>{t('settings.outputDevice')}</h3>

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
              <span class="setting-label">{t('settings.device')}</span>
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

            {/* A build without the ASIO backend is a dev build (--no-asio), not a
                shipping one, so this is the only case where the picker's absence
                of drivers has to be explained: on a real build, an empty ASIO
                half means the machine has no driver installed. */}
            <Show when={!asioCompiled()}>
              <div class="setting-hint">{t('settings.asioMissing')}</div>
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
            <h3>{t('settings.library')}</h3>

            <Show
              when={props.activeLibrary}
              fallback={<div class="setting-hint">{t('settings.noLibrary')}</div>}
            >
              {(library) => (
                <>
                  <div class="setting">
                    <span class="setting-label">{library().name}</span>
                    <span class="setting-hint">
                      {t('settings.soundCount', { count: library().count.toLocaleString() })} ·{' '}
                      {library().root}
                    </span>
                  </div>
                  <div class="setting">
                    <span class="setting-label">{t('settings.revealIndex')}</span>
                    <ix-button
                      class="slim-button"
                      variant="secondary"
                      onClick={() => void api.revealInFolder(library().root)}
                    >
                      {t('settings.reveal')}
                    </ix-button>
                  </div>
                </>
              )}
            </Show>

            <div class="setting">
              <span class="setting-label">{t('settings.addLibrary')}</span>
              <ix-button class="slim-button" variant="secondary" onClick={props.onAddLibrary}>
                {t('settings.openLibrary')}
              </ix-button>
            </div>

            <div class="setting">
              <span class="setting-label">{t('settings.showCategory')}</span>
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
            <h3>{t('settings.behaviour')}</h3>

            <div class="setting">
              <span class="setting-label">{t('settings.autoPlay')}</span>
              <ix-toggle
                checked={props.settings.autoPlay}
                on:checkedChange={(e: CustomEvent<boolean>) => props.onChange({ autoPlay: e.detail })}
              />
            </div>

          </section>

          {/* ------------------------------------------------------ appearance */}
          <section class="settings-section">
            <h3>{t('settings.appearance')}</h3>
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

            {/* Interface scale, then language. Both change the whole window rather
                than one pane, which is why neither sits under List or Behaviour. */}
            <div class="setting">
              <span class="setting-label">{t('settings.zoom')}</span>
              <div style={{ width: '140px' }}>
                <ix-select
                  value={String(props.zoom)}
                  allowClear={false}
                  on:valueChange={(e: CustomEvent<string | string[]>) =>
                    props.onZoom(Number(e.detail))
                  }
                >
                  <For each={ZOOM_STEPS}>
                    {(step) => <ix-select-item value={String(step)} label={`${step}%`} />}
                  </For>
                </ix-select>
              </div>
            </div>

            <div class="setting">
              <span class="setting-label">{t('settings.language')}</span>
              <div style={{ width: '160px' }}>
                <ix-select
                  value={locale()}
                  allowClear={false}
                  on:valueChange={(e: CustomEvent<string | string[]>) =>
                    setLocale(String(e.detail) as Locale)
                  }
                >
                  <For each={LOCALES}>
                    {(entry) => <ix-select-item value={entry.id} label={entry.label} />}
                  </For>
                </ix-select>
              </div>
            </div>
          </section>
        </div>
      </ix-modal-content>
      <ix-modal-footer>
        <div class="modal-actions">
          <div class="spacer" />
          <ix-button variant="primary" onClick={close}>
            {t('common.done')}
          </ix-button>
        </div>
      </ix-modal-footer>
    </ix-modal>
  )
}
