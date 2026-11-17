import { Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js'
import { api, type SoundRow } from './ipc'
import { categoryName, channelLabel, t } from './i18n'
import { ICON } from './icons'

interface Props {
  row: SoundRow | null
  positionS: number
  durationS: number
  playing: boolean
  /** Loaded but frozen: the Pause button becomes Resume and the playhead holds. */
  paused: boolean
  /** How many rows the selection covers, for the dock's header line. */
  selectionCount: number
  /** Auto-play on select: whether clicking a row auditions it. */
  autoPlay: boolean
  /** Output gain, `0..1`. */
  gain: number
  onStop: () => void
  onToggle: () => void
  onToggleAutoPlay: () => void
  onGain: (gain: number) => void
  onSeek: (seconds: number) => void
  /** Decode cost of the waveform, reported to the status bar only. */
  onDecoded: (ms: number) => void
}

const MIN_H = 64
const MAX_H = 260

/** Slider range. 1% is fine enough for a level control and keeps the readout steady. */
const GAIN_MAX = 100

/**
 * The waveform dock.
 *
 * Draws ~1000 peaks from the backend on a canvas. No audio data ever reaches this
 * component: `AGENTS.md` says the renderer never touches PCM, so the only numbers
 * here are the peak envelope and the playhead position.
 *
 * The playhead is drawn on a second canvas layered over the peaks, and only that
 * layer is repainted on each animation frame — repainting a 1000-bar waveform at
 * 60Hz to move one line by a pixel is wasted work on a machine that is also
 * running the audio callback.
 */
export function WaveformDock(props: Props) {
  const [peaks, setPeaks] = createSignal<number[] | null>(null)
  const [loadError, setLoadError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [height, setHeight] = createSignal(120)

  let waveCanvas: HTMLCanvasElement | undefined
  let headCanvas: HTMLCanvasElement | undefined

  // A new row invalidates whatever waveform is on screen immediately, so the dock
  // never shows the previous sample's shape under the new sample's name.
  createEffect(() => {
    const row = props.row
    setPeaks(null)
    setLoadError(null)
    if (!row) return

    let cancelled = false
    setLoading(true)
    api
      .waveform(row.path)
      .then((w) => {
        if (cancelled) return
        setPeaks(w.peaks)
        props.onDecoded(w.decodeMs)
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    onCleanup(() => {
      cancelled = true
    })
  })

  /** A CSS variable resolved on the document, for the two canvas colours. */
  const token = (name: string, fallback: string): string => {
    const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
    return value || fallback
  }

  const drawPeaks = () => {
    const canvas = waveCanvas
    const data = peaks()
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (w === 0 || h === 0) return
    canvas.width = Math.round(w * dpr)
    canvas.height = Math.round(h * dpr)
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

    ctx.clearRect(0, 0, w, h)
    const mid = h / 2

    // No background fill: the container already has the panel colour, and filling
    // it here is what turned the whole dock one colour on the first run.
    if (!data || data.length === 0) return

    // A one-pixel zero line, so an all-silent sample reads as a flat line rather
    // than as an empty panel.
    ctx.fillStyle = token('--theme-color-3', '#3c484d')
    ctx.fillRect(0, Math.round(mid), w, 1)

    const barW = w / data.length
    ctx.fillStyle = token('--mio-wave', '#00eaff')
    for (let i = 0; i < data.length; i++) {
      const peak = data[i] ?? 0
      const barH = Math.max(1, peak * (h - 4))
      ctx.fillRect(i * barW, mid - barH / 2, Math.max(1, barW - 0.5), barH)
    }
  }

  const drawHead = () => {
    const canvas = headCanvas
    const data = peaks()
    if (!canvas || !data) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (w === 0 || h === 0) return
    canvas.width = Math.round(w * dpr)
    canvas.height = Math.round(h * dpr)
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)

    const duration = props.durationS
    if (duration <= 0) return
    const fraction = Math.min(1, Math.max(0, props.positionS / duration))
    const x = fraction * w
    ctx.fillStyle = token('--mio-playhead', '#ffffff')
    ctx.fillRect(x - 0.5, 0, 1, h)
  }

  createEffect(() => {
    // Re-read the reactive dependencies before each paint so Solid tracks them.
    props.positionS
    props.durationS
    peaks()
    drawHead()
  })

  createEffect(() => {
    peaks()
    height()
    drawPeaks()
  })

  /**
   * Repaint when the theme changes.
   *
   * The canvases are painted once and then left alone, so a theme switch would
   * leave the old palette on screen — the peaks layer is never repainted at all.
   * A MutationObserver on the root's theme attributes is the cheapest honest
   * signal that the tokens moved.
   */
  onMount(() => {
    const observer = new MutationObserver(() => {
      drawPeaks()
      drawHead()
    })
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-mio-theme', 'data-ix-color-schema', 'style']
    })
    onCleanup(() => observer.disconnect())
  })

  const startResize = (event: PointerEvent) => {
    event.preventDefault()
    const startY = event.clientY
    const startH = height()
    const move = (e: PointerEvent) => {
      const next = Math.min(MAX_H, Math.max(MIN_H, startH - (e.clientY - startY)))
      setHeight(next)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const seek = (event: MouseEvent) => {
    const canvas = waveCanvas
    if (!canvas || props.durationS <= 0) return
    const rect = canvas.getBoundingClientRect()
    const fraction = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width))
    props.onSeek(fraction * props.durationS)
  }

  const label = () => {
    if (!props.row) return ''
    const mm = Math.floor(props.positionS / 60)
    const ss = (props.positionS % 60).toFixed(2).padStart(5, '0')
    const dm = Math.floor(props.durationS / 60)
    const ds = (props.durationS % 60).toFixed(2).padStart(5, '0')
    return `${mm}:${ss} / ${dm}:${ds}`
  }

  /** Slider units (`0..100`) to gain. */
  const gainFromSlider = (value: number) => Math.max(0, Math.min(1, value / GAIN_MAX))

  return (
    <section class="dock">
      <div class="dock-resize" onPointerDown={startResize} />

      <div class="dock-head">
        <Show when={props.row} fallback={<span class="dim">—</span>}>
          {(row) => (
            <>
              <span class="dock-name" title={row().path}>
                {row().filename}
              </span>
              <span class="dim">
                {categoryName(row().category)}
                <Show when={row().subcategory}> / {row().subcategory}</Show>
              </span>
              <span class="dim">{row().durationLabel}</span>
              <span class="dim">
                {row().sampleRate} Hz · {channelLabel(row().channels)}
              </span>
              <Show when={props.selectionCount > 1}>
                <span class="dim">{t('status.selected', { count: props.selectionCount })}</span>
              </Show>
            </>
          )}
        </Show>

        <div class="spacer" />

        {/*
          Volume, immediately left of AUTO PLAY. A mute toggle plus the short
          slider, in one cluster: they change the same thing, so they read as one
          control. Never disabled — the level belongs to the output, not to the
          clip, so it is settable with nothing loaded.
        */}
        <div class="volume" title={t('dock.outputLevel')}>
          <ix-icon-button
            id="volume-mute"
            size="24"
            variant="tertiary"
            icon={props.gain === 0 ? ICON.mute : ICON.sound}
            title={props.gain === 0 ? t('dock.unmute') : t('dock.mute')}
            aria-label={props.gain === 0 ? t('dock.unmute') : t('dock.mute')}
            onClick={() => props.onGain(props.gain === 0 ? 1 : 0)}
          />
          <div class="volume-slider">
            <ix-slider
              id="volume-slider"
              min={0}
              max={GAIN_MAX}
              step={1}
              value={Math.round(props.gain * GAIN_MAX)}
              trace
              aria-label={t('dock.outputLevel')}
              on:valueChange={(e: CustomEvent<number>) => props.onGain(gainFromSlider(e.detail))}
            />
          </div>
          <span class="volume-value">{Math.round(props.gain * 100)}%</span>
        </div>

        {/*
          AUTO PLAY leads the transport, then Pause/Resume and Stop, so the three
          controls that decide whether a click makes sound sit in one cluster
          instead of being split between the top bar and the dock. It is a word
          rather than a bolt glyph: the state is a policy ("does clicking a row
          make sound"), and a 12px icon cannot say that.
        */}
        <ix-toggle-button
          id="auto-play"
          variant="secondary"
          pressed={props.autoPlay}
          title={`${props.autoPlay ? t('dock.autoPlayOn') : t('dock.autoPlayOff')} — ${t('dock.autoPlayHint')}`}
          aria-label={props.autoPlay ? t('dock.autoPlayOn') : t('dock.autoPlayOff')}
          on:pressedChange={() => props.onToggleAutoPlay()}
        >
          {t('dock.autoPlayLabel')}
        </ix-toggle-button>

        <ix-icon-button
          id="transport-toggle"
          size="24"
          variant="tertiary"
          icon={props.paused ? ICON.play : ICON.pause}
          disabled={!props.playing && !props.paused}
          title={props.paused ? t('dock.resume') : t('dock.pause')}
          aria-label={props.paused ? t('dock.resumeLabel') : t('dock.pauseLabel')}
          onClick={props.onToggle}
        />

        <ix-icon-button
          id="transport-stop"
          size="24"
          variant="tertiary"
          icon={ICON.stop}
          disabled={!props.playing && !props.paused}
          title={t('dock.stop')}
          aria-label={t('dock.stopLabel')}
          onClick={props.onStop}
        />
      </div>

      <div class="wave-wrap" style={{ height: `${height()}px` }}>
        <canvas ref={waveCanvas} class="wave" onClick={seek} />
        <canvas ref={headCanvas} class="wave-head" onClick={seek} />

        <Show when={loading()}>
          <div class="wave-overlay">{t('dock.decoding')}</div>
        </Show>
        <Show when={loadError()}>{(err) => <div class="wave-overlay bad">{err()}</div>}</Show>

        <Show when={props.row}>
          <div class="wave-time">{label()}</div>
        </Show>
      </div>
    </section>
  )
}
