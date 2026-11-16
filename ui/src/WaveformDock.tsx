import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import { api, type SoundRow } from './ipc'
import { PauseIcon, PlayIcon, StopIcon } from './icons'

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
  onStop: () => void
  onToggle: () => void
  onToggleAutoPlay: () => void
  onSeek: (seconds: number) => void
  /** Decode cost of the waveform, reported to the status bar only. */
  onDecoded: (ms: number) => void
}

const MIN_H = 64
const MAX_H = 260

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
  let container: HTMLDivElement | undefined

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
    // it here is what turned the whole dock blue on the first run.
    if (!data || data.length === 0) return

    // A one-pixel zero line, so an all-silent sample reads as a flat line rather
    // than as an empty panel.
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--border-strong')
    ctx.fillRect(0, Math.round(mid), w, 1)

    const barW = w / data.length
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--wave')
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
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--playhead')
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

  return (
    <section class="dock" ref={container}>
      <div class="dock-resize" onPointerDown={startResize} />

      <div class="dock-head">
        <Show when={props.row} fallback={<span class="dim">—</span>}>
          {(row) => (
            <>
              <span class="dock-name" title={row().path}>
                {row().filename}
              </span>
              <span class="dim">
                {row().category}
                <Show when={row().subcategory}> / {row().subcategory}</Show>
              </span>
              <span class="dim">{row().durationLabel}</span>
              <span class="dim">
                {row().sampleRate} Hz · {row().channelLabel}
              </span>
              <Show when={props.selectionCount > 1}>
                <span class="dim">{props.selectionCount} selected</span>
              </Show>
            </>
          )}
        </Show>

        <div class="spacer" />


        {/*
          Icon-only transport. The auto-play toggle leads, then Pause/Resume and
          Stop, so the three controls that decide whether a click makes sound sit
          in one cluster instead of being split between the top bar and the dock.
          Each one carries its label in `title` and `aria-label`: with no text on
          screen, those are the only names a screen reader or a tooltip has.
        */}
        <button
          class={props.autoPlay ? 'ghost auto on' : 'ghost auto'}
          onClick={props.onToggleAutoPlay}
          title={`Auto-play ${props.autoPlay ? 'on' : 'off'} — clicking a row auditions it (Alt+A)`}
          aria-label={`Auto-play ${props.autoPlay ? 'on' : 'off'}`}
          aria-pressed={props.autoPlay}
        >
          AUTO PLAY
        </button>

        <button
          class="ghost icon"
          onClick={props.onToggle}
          disabled={!props.playing && !props.paused}
          title={props.paused ? 'Resume (Space)' : 'Pause (Space)'}
          aria-label={props.paused ? 'Resume' : 'Pause'}
        >
          <Show when={props.paused} fallback={<PauseIcon />}>
            <PlayIcon />
          </Show>
        </button>

        <button
          class="ghost icon"
          onClick={props.onStop}
          disabled={!props.playing && !props.paused}
          title="Stop and clear the playhead"
          aria-label="Stop"
        >
          <StopIcon />
        </button>
      </div>

      <div class="wave-wrap" style={{ height: `${height()}px` }}>
        <canvas ref={waveCanvas} class="wave" onClick={seek} />
        <canvas ref={headCanvas} class="wave-head" onClick={seek} />

        <Show when={loading()}>
          <div class="wave-overlay">decoding…</div>
        </Show>
        <Show when={loadError()}>
          {(err) => <div class="wave-overlay bad">{err()}</div>}
        </Show>

        <Show when={props.row}>
          <div class="wave-time">{label()}</div>
        </Show>
      </div>
    </section>
  )
}
