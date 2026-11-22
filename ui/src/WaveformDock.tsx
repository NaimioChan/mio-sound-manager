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
  /**
   * Drag-out, started from the top third of the waveform: hand the selection (or
   * the whole clip) to the OS as a file drop.
   *
   * `atSeconds` is where inside the panel the gesture began. The app decides what
   * that means — a point inside the selection drags the selection, anywhere else
   * drags the source file — because the selection is app state and the panel only
   * knows the pointer position.
   */
  onDragOut: (atSeconds: number) => void
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
 * The share of the panel's height that drags out instead of selecting.
 *
 * The top third is the handle: pressing there takes the material out of the app,
 * pressing anywhere below it draws a selection. The split is spatial rather than
 * a modifier key because the gesture a DAW user makes is one continuous motion —
 * hold, drag, release over the other window — and a chord has to be re-established
 * every time the pointer leaves.
 */
const DRAG_OUT_BAND = 1 / 3

/**
 * How close to a region edge a press counts as grabbing that edge, in pixels.
 *
 * Wide enough to hit with a mouse, narrow enough that the middle of a short
 * selection still draws a new one.
 */
const EDGE_HIT_PX = 5

/** `m:ss.cc`, the same shape the playhead readout uses. */
const fmtTime = (seconds: number): string => {
  const m = Math.floor(seconds / 60)
  const s = (seconds % 60).toFixed(2).padStart(5, '0')
  return `${m}:${s}`
}

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
  /** One envelope per channel for mono/stereo files; empty for wider ones. */
  const [lanes, setLanes] = createSignal<number[][]>([])
  const [loadError, setLoadError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [height, setHeight] = createSignal(120)
  /** The region being dragged out, before the pointer is released. */
  const [draft, setDraft] = createSignal<Region | null>(null)
  /** True while the pointer is down on the waveform, for the cursor. */
  const [selecting, setSelecting] = createSignal(false)
  /** True while the pointer is over the drag-out band, so the panel says so. */
  const [grabBand, setGrabBand] = createSignal(false)
  /** True while a region edge is being dragged. */
  const [edgeDrag, setEdgeDrag] = createSignal(false)
  /** Which edge the pointer is over, for the resize cursor. */
  const [hoverEdge, setHoverEdge] = createSignal<'start' | 'end' | null>(null)

  let waveCanvas: HTMLCanvasElement | undefined
  let headCanvas: HTMLCanvasElement | undefined

  // A new row invalidates whatever waveform is on screen immediately, so the dock
  // never shows the previous sample's shape under the new sample's name.
  createEffect(() => {
    const row = props.row
    setPeaks(null)
    setLanes([])
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
        setLanes(w.lanes)
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

  /**
   * Theme tokens, cached.
   *
   * `getComputedStyle` was called four times per animation frame to resolve the
   * same handful of variables; the canvas colours only move when the theme
   * moves, and the theme observer below already knows when that happens. The
   * cache is invalidated there and filled lazily on first read.
   */
  let tokenCache: Record<string, string> | null = null
  const tokens = (): Record<string, string> => {
    if (!tokenCache) {
      const style = getComputedStyle(document.documentElement)
      const names = [
        '--mio-wave',
        '--theme-color-3',
        '--mio-region-scrim',
        '--mio-region-fill',
        '--mio-region-edge',
        '--mio-playhead'
      ]
      const table: Record<string, string> = {}
      for (const name of names) {
        const value = style.getPropertyValue(name).trim()
        if (value) table[name] = value
      }
      tokenCache = table
    }
    return tokenCache
  }

  const drawPeaks = () => {
    const canvas = waveCanvas
    const data = peaks()
    if (!canvas) return
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (w === 0 || h === 0) return
    const dpr = window.devicePixelRatio || 1
    // Same rule as the head layer: assigning width/height reallocates the
    // backing store, so it only happens on a real size change.
    const pxW = Math.round(w * dpr)
    const pxH = Math.round(h * dpr)
    if (canvas.width !== pxW || canvas.height !== pxH) {
      canvas.width = pxW
      canvas.height = pxH
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    ctx.clearRect(0, 0, canvas.width, canvas.height)

    // No background fill: the container already has the panel colour, and filling
    // it here is what turned the whole dock one colour on the first run.
    if (!data || data.length === 0) return

    // Everything below is in device pixels, with no `setTransform`. That is the
    // difference between a solid waveform and a striped one: at 125% scaling a
    // 0.993px CSS bar sits on a fractional device column, so every bar is drawn as
    // two half-covered columns and the panel reads as alternating light and dark
    // stripes. Snapping each bucket's edges to whole device pixels makes adjacent
    // buckets tile exactly, and one colour stays one colour.
    const wave = tokens()['--mio-wave'] ?? '#00eaff'
    const zero = tokens()['--theme-color-3'] ?? '#3c484d'
    // One lane per channel for mono and stereo; a wider file keeps the merged
    // envelope in a single lane, because a third strip would be too thin to read.
    const stacked = lanes()
    const rows = stacked.length > 0 ? stacked : [data]

    for (let lane = 0; lane < rows.length; lane++) {
      const series = rows[lane]!
      const laneTop = Math.round((lane * pxH) / rows.length)
      const laneBottom = Math.round(((lane + 1) * pxH) / rows.length)
      const laneH = laneBottom - laneTop
      const mid = Math.round(laneTop + laneH / 2)

      // A one-pixel zero line per lane, so an all-silent sample reads as a flat
      // line rather than as an empty panel.
      ctx.fillStyle = zero
      ctx.fillRect(0, mid, pxW, 1)

      const barW = pxW / series.length
      ctx.fillStyle = wave
      for (let i = 0; i < series.length; i++) {
        const peak = series[i] ?? 0
        const x0 = Math.round(i * barW)
        const x1 = Math.round((i + 1) * barW)
        // 2px of headroom inside the lane so adjacent lanes never touch.
        const barH = Math.max(1, Math.round(peak * (laneH - 2)))
        const top = Math.round(mid - barH / 2)
        ctx.fillRect(x0, top, Math.max(1, x1 - x0), barH)
      }
    }
  }

  /** The region to draw: the live draft while dragging, otherwise the committed one. */
  const shownRegion = (): Region | null => draft() ?? props.region

  const drawHead = () => {
    const canvas = headCanvas
    const data = peaks()
    if (!canvas || !data) return
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (w === 0 || h === 0) return
    const dpr = window.devicePixelRatio || 1
    // Assigning width/height resets the backing store: doing it every frame
    // reallocates the canvas under the playhead 60 times a second. Only a real
    // size (or scaling) change may reset it.
    const pxW = Math.round(w * dpr)
    const pxH = Math.round(h * dpr)
    if (canvas.width !== pxW || canvas.height !== pxH) {
      canvas.width = pxW
      canvas.height = pxH
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, canvas.width, canvas.height)

    const duration = props.durationS
    if (duration <= 0) return

    // The selection is marked by taking light *away* from everything else, not by
    // tinting what is inside it. A translucent accent over bright bars is the
    // weakest possible mark: bars are already the accent's colour family, so on a
    // loud clip the selection disappeared entirely. Darkening the outside works at
    // any bar density, and the two edges say exactly where it begins and ends
    // because a fill alone cannot on a waveform that is nothing but fill.
    const region = shownRegion()
    if (region) {
      const x0 = Math.round((region[0] / duration) * pxW)
      const x1 = Math.round((region[1] / duration) * pxW)
      ctx.fillStyle = tokens()['--mio-region-scrim'] ?? 'rgba(15,22,25,0.62)'
      ctx.fillRect(0, 0, x0, pxH)
      ctx.fillRect(x1, 0, pxW - x1, pxH)

      ctx.fillStyle = tokens()['--mio-region-fill'] ?? 'rgba(0,189,227,0.10)'
      ctx.fillRect(x0, 0, Math.max(1, x1 - x0), pxH)

      ctx.fillStyle = tokens()['--mio-region-edge'] ?? '#00bde3'
      const edge = Math.max(2, Math.round(dpr))
      ctx.fillRect(x0, 0, edge, pxH)
      ctx.fillRect(Math.max(0, x1 - edge), 0, edge, pxH)
    }

    const fraction = Math.min(1, Math.max(0, props.positionS / duration))
    const x = Math.round(fraction * pxW)
    const headW = Math.max(1, Math.round(dpr))
    ctx.fillStyle = tokens()['--mio-playhead'] ?? '#ffffff'
    ctx.fillRect(x - Math.floor(headW / 2), 0, headW, pxH)
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
    lanes()
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
      // The theme moved, so the cached canvas colours are stale.
      tokenCache = null
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
    // A gesture interrupted by unmount (a library switch, an index pass) would
    // otherwise leave the listeners on the window for the life of the page.
    onCleanup(() => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    })
  }

  /** Canvas x → seconds, clamped to the clip. */
  const secondsAt = (clientX: number): number => {
    const canvas = waveCanvas
    if (!canvas || props.durationS <= 0) return 0
    const rect = canvas.getBoundingClientRect()
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
    return fraction * props.durationS
  }

  /** Whether a pointer at `clientY` is in the band that drags material out. */
  const inGrabBand = (clientY: number): boolean => {
    const canvas = waveCanvas
    if (!canvas) return false
    const rect = canvas.getBoundingClientRect()
    if (rect.height === 0) return false
    return (clientY - rect.top) / rect.height <= DRAG_OUT_BAND
  }

  /**
   * Which region edge a pointer is on, if any.
   *
   * 5px either side of the edge, and only when the pointer is outside the
   * drag-out band: the band is the whole top third of the panel, so a region whose
   * edge happens to sit under it must not steal the grab.
   */
  const edgeAt = (clientX: number, clientY: number): 'start' | 'end' | null => {
    const canvas = waveCanvas
    const region = props.region
    if (!canvas || !region || props.durationS <= 0 || inGrabBand(clientY)) return null
    const rect = canvas.getBoundingClientRect()
    if (rect.width === 0) return null
    const x0 = rect.left + (region[0] / props.durationS) * rect.width
    const x1 = rect.left + (region[1] / props.durationS) * rect.width
    // The nearer edge wins when the region is narrower than two hit zones, so a
    // very short selection is still adjustable instead of being a dead zone.
    const toStart = Math.abs(clientX - x0)
    const toEnd = Math.abs(clientX - x1)
    if (toStart <= EDGE_HIT_PX && toStart <= toEnd) return 'start'
    if (toEnd <= EDGE_HIT_PX) return 'end'
    return null
  }

  /** Hover state: the band highlight and the edge cursor both come from here. */
  const onPointerMove = (event: PointerEvent) => {
    if (selecting() || edgeDrag()) return
    setGrabBand(inGrabBand(event.clientY))
    setHoverEdge(edgeAt(event.clientX, event.clientY))
  }

  const onPointerLeave = () => {
    setGrabBand(false)
    setHoverEdge(null)
  }

  /**
   * One pointer handler for every gesture the waveform has.
   *
   * Where the press lands decides what happens, and there are no modifier keys:
   *
   * - top third → drag out, as a file drop (the band highlights on hover)
   * - on a region edge → move that edge
   * - anywhere else → press and hold draws a selection; a press that does not
   *   move seeks
   *
   * Doing it from `pointerdown` rather than `click` is what makes the selection
   * feel attached to the pointer: the region grows while the button is held, and a
   * plain click still lands on the exact frame it always did.
   */
  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0 || !props.row || props.durationS <= 0) return

    // Drag out first: the OS drag has to begin with the button down, so this is
    // the only moment it can start.
    if (inGrabBand(event.clientY)) {
      event.preventDefault()
      props.onDragOut(secondsAt(event.clientX))
      return
    }

    const edge = edgeAt(event.clientX, event.clientY)
    if (edge) {
      event.preventDefault()
      const region = props.region
      if (!region) return
      setEdgeDrag(true)
      setSelecting(true)
      const anchor = edge === 'start' ? region[1] : region[0]
      const move = (e: PointerEvent) => {
        const to = secondsAt(e.clientX)
        const from = Math.min(anchor, to)
        let end = Math.max(anchor, to)
        if (end - from < MIN_REGION_S) {
          end = Math.min(props.durationS, from + MIN_REGION_S)
        }
        setDraft([from, end])
      }
      const up = (e: PointerEvent) => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        setEdgeDrag(false)
        setSelecting(false)
        setDraft(null)
        const to = secondsAt(e.clientX)
        const from = Math.min(anchor, to)
        let end = Math.max(anchor, to)
        if (end - from < MIN_REGION_S) {
          end = Math.min(props.durationS, from + MIN_REGION_S)
        }
        props.onRegion([from, end])
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
      // Same unmount guard as the resize gesture: the listeners live on the
      // window, so they would outlive the component that started them.
      onCleanup(() => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
      })
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
    onCleanup(() => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    })
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
    return `${fmtTime(region[0])} – ${fmtTime(region[1])} · ${(region[1] - region[0]).toFixed(2)} s`
  }

  /**
   * What the loop switch is currently looping, as a sentence fragment.
   *
   * The switch alone cannot say this: "on" looks the same whether it repeats a
   * 400ms selection or the whole three-minute file, and those are very different
   * things to have armed. So the state is spelled out in the tooltip and in the
   * accessible name, and the switch's filled style carries the on/off half.
   */
  const loopState = (): string => {
    if (!props.looping) return t('dock.loopOff')
    const region = props.region
    if (!region) return `${t('dock.loopOn')} · ${t('dock.loopWhole')}`
    return `${t('dock.loopOn')} · ${fmtTime(region[0])} – ${fmtTime(region[1])}`
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

          `ix-icon-toggle-button`, not `ix-icon-button`: iX 5's icon button hard-codes
          `selected: false` in its render, so the `selected` prop this used to pass
          reached nothing and the switch had no on state at all. The toggle variant
          forwards `pressed` to the same `.selected` class, and adds `aria-pressed`.
        */}
        <ix-icon-toggle-button
          id="transport-loop"
          classList={{ on: props.looping }}
          size="24"
          variant="tertiary"
          icon={ICON.loop}
          pressed={props.looping}
          title={`${loopState()} — ${t('dock.loopHint')}`}
          aria-label={loopState()}
          on:pressedChange={() => props.onToggleLoop()}
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
        classList={{
          selecting: selecting(),
          'has-region': shownRegion() !== null,
          grab: grabBand(),
          'edge-start': hoverEdge() === 'start' && !selecting(),
          'edge-end': hoverEdge() === 'end' && !selecting(),
          'edge-drag': edgeDrag()
        }}
        style={{ height: `${height()}px` }}
      >
        <canvas
          ref={waveCanvas}
          class="wave"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerLeave={onPointerLeave}
        />
        <canvas ref={headCanvas} class="wave-head" />

        {/* The drag-out band, drawn on its own layer so it appears on hover without
            repainting the peaks. It says where the handle is; nothing else does. */}
        <div class="wave-grab" aria-hidden="true" />

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

        Only the gestures that cannot be discovered by trying them belong here: a
        drag that selects and a drag that hands the file to another window look
        identical until the pointer is over the panel, and the band is what tells
        them apart. Kept to one line — this is a control legend, not documentation.
      */}
      <div class="dock-hints">
        <span>
          <kbd>{t('hint.drag')}</kbd> {t('hint.select')}
        </span>
        <span>
          <kbd>{t('hint.click')}</kbd> {t('hint.seek')}
        </span>
        <span>
          <kbd>{t('hint.dragTop')}</kbd> {t('hint.dragOut')}
        </span>
        <span>
          <kbd>{t('hint.dragEdge')}</kbd> {t('hint.resizeRegion')}
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
