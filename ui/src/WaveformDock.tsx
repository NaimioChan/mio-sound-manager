import { Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js'
import { api, type SoundRow } from './ipc'
import { categoryName, channelLabel, t } from './i18n'
import { ICON } from './icons'

/** A region of the clip, in seconds, half-open: `[start, end)`. */
export type Region = [number, number]

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
  /** The selected region, owned by the app so the loop and Esc can read it. */
  region: Region | null
  /** Whether the engine is looping. */
  looping: boolean
  /** True while a selection is being written out for a drag, so the panel says so. */
  exporting: boolean
  onStop: () => void
  onToggle: () => void
  onToggleAutoPlay: () => void
  onToggleMute: () => void
  onToggleLoop: () => void
  onGain: (gain: number) => void
  onSeek: (seconds: number) => void
  onRegion: (region: Region | null) => void
  /** Alt+press: hand the selection (or the whole clip) to the OS as a file drop. */
  onDragOut: () => void
  /** Decode cost of the waveform, reported to the status bar only. */
  onDecoded: (ms: number) => void
}

const MIN_H = 64
const MAX_H = 260

/** Slider range. 1% is fine enough for a level control and keeps the readout steady. */
const GAIN_MAX = 100

/**
 * Pixels a pointer must travel before a press becomes a selection.
 *
 * Below this a press is a click, and a click on the waveform seeks — which is the
 * behaviour the dock had before selection existed and still the one a user
 * expects when they aim at a spot. Without the threshold every seek would leave a
 * zero-width region behind.
 */
const DRAG_SLOP = 3

/**
 * Shortest region a drag can produce, in seconds.
 *
 * A 2px drag on a 3-minute file is a few milliseconds of audio, which is not a
 * usable loop and not something worth writing out as a WAV. Clamped rather than
 * rejected: the user did mean to select something.
 */
const MIN_REGION_S = 0.01

/**
 * The waveform dock.
 *
 * Draws ~1000 peaks from the backend on a canvas. No audio data ever reaches this
 * component: `AGENTS.md` says the renderer never touches PCM, so the only numbers
 * here are the peak envelope and the playhead position.
 *
 * The playhead and the selection are drawn on a second canvas layered over the
 * peaks, and only that layer is repainted on each animation frame — repainting a
 * 1000-bar waveform at 60Hz to move one line by a pixel is wasted work on a
 * machine that is also running the audio callback.
 */
export function WaveformDock(props: Props) {
  const [peaks, setPeaks] = createSignal<number[] | null>(null)
  const [loadError, setLoadError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [height, setHeight] = createSignal(120)
  /** The region being dragged out, before the pointer is released. */
  const [draft, setDraft] = createSignal<Region | null>(null)
  /** True while the pointer is down on the waveform, for the cursor. */
  const [selecting, setSelecting] = createSignal(false)

  let waveCanvas: HTMLCanvasElement | undefined
  let headCanvas: HTMLCanvasElement | undefined

  // A new row invalidates whatever waveform is on screen immediately, so the dock
  // never shows the previous sample's shape under the new sample's name.
  createEffect(() => {
    const row = props.row
    setPeaks(null)
    setLoadError(null)
    setDraft(null)
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

  /** The region to draw: the live draft while dragging, otherwise the committed one. */
  const shownRegion = (): Region | null => draft() ?? props.region

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

    // The selection goes under the playhead so the playhead stays visible inside
    // it. A translucent fill plus two edges: the fill says "this part", the edges
    // say exactly where it starts and stops, which a fill alone cannot on a
    // waveform whose bars are already the same colour family.
    const region = shownRegion()
    if (region) {
      const x0 = (region[0] / duration) * w
      const x1 = (region[1] / duration) * w
      ctx.globalAlpha = 0.16
      ctx.fillStyle = token('--theme-color-primary', '#00bde3')
      ctx.fillRect(x0, 0, Math.max(1, x1 - x0), h)
      ctx.globalAlpha = 1
      ctx.fillRect(x0, 0, 1, h)
      ctx.fillRect(Math.max(0, x1 - 1), 0, 1, h)
    }

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
    shownRegion()
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

  /** Canvas x → seconds, clamped to the clip. */
  const secondsAt = (clientX: number): number => {
    const canvas = waveCanvas
    if (!canvas || props.durationS <= 0) return 0
    const rect = canvas.getBoundingClientRect()
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return fraction * props.durationS
  }

  /**
   * One pointer handler for both gestures the waveform has.
   *
   * A press that does not move seeks; a press that moves selects. Doing it from
   * `pointerdown` rather than `click` is what makes the selection feel attached to
   * the pointer: the region grows while the button is held, and a plain click
   * still lands on the exact frame it always did.
   */
  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0 || !props.row || props.durationS <= 0) return

    // Alt+press is "take this out of the app": the gesture a DAW user reaches for
    // when they want the material, not playback. Started from the press for the
    // same reason the row drag is — the OS drag has to begin with the button down.
    if (event.altKey) {
      event.preventDefault()
      props.onDragOut()
      return
    }

    event.preventDefault()
    const anchor = secondsAt(event.clientX)
    const startX = event.clientX
    let moved = false
    setSelecting(true)

    const move = (e: PointerEvent) => {
      if (!moved && Math.abs(e.clientX - startX) < DRAG_SLOP) return
      moved = true
      const to = secondsAt(e.clientX)
      setDraft([Math.min(anchor, to), Math.max(anchor, to)])
    }

    const up = (e: PointerEvent) => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      setSelecting(false)
      setDraft(null)

      if (!moved) {
        // A click, not a drag: seek to where the pointer is.
        props.onSeek(secondsAt(e.clientX))
        return
      }
      const to = secondsAt(e.clientX)
      let from = Math.min(anchor, to)
      let end = Math.max(anchor, to)
      if (end - from < MIN_REGION_S) end = Math.min(props.durationS, from + MIN_REGION_S)
      if (end - from < MIN_REGION_S) from = Math.max(0, end - MIN_REGION_S)
      props.onRegion([from, end])
    }

    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const label = () => {
    if (!props.row) return ''
    const mm = Math.floor(props.positionS / 60)
    const ss = (props.positionS % 60).toFixed(2).padStart(5, '0')
    const dm = Math.floor(props.durationS / 60)
    const ds = (props.durationS % 60).toFixed(2).padStart(5, '0')
    return `${mm}:${ss} / ${dm}:${ds}`
  }

  /** `0:01.20 – 0:02.40 · 1.20 s`, or `null` with nothing selected. */
  const regionLabel = (): string | null => {
    const region = shownRegion()
    if (!region) return null
    const fmt = (seconds: number) => {
      const m = Math.floor(seconds / 60)
      const s = (seconds % 60).toFixed(2).padStart(5, '0')
      return `${m}:${s}`
    }
    return `${fmt(region[0])} – ${fmt(region[1])} · ${(region[1] - region[0]).toFixed(2)} s`
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
            onClick={props.onToggleMute}
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
          AUTO PLAY leads the transport, then Loop, Pause/Resume and Stop, so the
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

        {/*
          Loop sits with the transport, not with the volume: it decides what gets
          played, and it reads the selection the waveform below holds. An icon
          rather than a word because the dock's head is already crowded and a loop
          is a recognised transport glyph.
        */}
        <ix-icon-button
          id="transport-loop"
          size="24"
          variant="tertiary"
          icon={ICON.loop}
          selected={props.looping}
          title={`${props.looping ? t('dock.loopOn') : t('dock.loopOff')} — ${t('dock.loopHint')}`}
          aria-label={props.looping ? t('dock.loopOn') : t('dock.loopOff')}
          onClick={props.onToggleLoop}
        />

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

      <div
        class="wave-wrap"
        classList={{ selecting: selecting(), 'has-region': shownRegion() !== null }}
        style={{ height: `${height()}px` }}
      >
        <canvas ref={waveCanvas} class="wave" onPointerDown={onPointerDown} />
        <canvas ref={headCanvas} class="wave-head" />

        <Show when={loading()}>
          <div class="wave-overlay">{t('dock.decoding')}</div>
        </Show>
        <Show when={loadError()}>{(err) => <div class="wave-overlay bad">{err()}</div>}</Show>
        <Show when={props.exporting}>
          <div class="wave-overlay dim">{t('dock.exporting')}</div>
        </Show>

        <Show when={props.row}>
          <div class="wave-time">{label()}</div>
        </Show>
        <Show when={regionLabel()}>
          {(text) => (
            <div class="wave-region" data-region="true">
              {text()}
              <button
                class="wave-region-clear"
                title={t('dock.clearRegion')}
                aria-label={t('dock.clearRegion')}
                onClick={() => props.onRegion(null)}
              >
                ×
              </button>
            </div>
          )}
        </Show>
      </div>

      {/*
        The mouse legend, under the waveform it describes.

        Every gesture here is invisible until it is tried, and the two that matter
        most — drag a region, Alt+drag to take it out — are not guessable from the
        panel. Kept to one line: this is a control legend, not documentation.
      */}
      <div class="dock-hints">
        <span>
          <kbd>{t('hint.drag')}</kbd> {t('hint.select')}
        </span>
        <span>
          <kbd>{t('hint.click')}</kbd> {t('hint.seek')}
        </span>
        <span>
          <kbd>Alt</kbd>+{t('hint.drag')} {t('hint.dragOut')}
        </span>
        <span>
          <kbd>Esc</kbd> {t('hint.clearRegion')}
        </span>
        <span>
          <kbd>L</kbd> {t('hint.loop')}
        </span>
      </div>
    </section>
  )
}
