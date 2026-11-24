import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { categoryName, channelLabel, t } from './i18n'
import { api, startDrag, type SearchRequest, type SoundRow } from './ipc'

/**
 * The sound list.
 *
 * Virtualised by row window rather than by a table library: the index can return
 * thousands of rows and the reference implementation built its own windowing too.
 * Fixed row height keeps the maths exact, which matters because a mis-measured
 * row shows up as a jump when scrolling.
 *
 * Selection is an *anchor plus a set*, the way Explorer does it: click replaces,
 * Ctrl+click toggles, Shift+click extends from the anchor. The anchor is a path
 * rather than an index because the list is re-sorted and re-queried underneath the
 * selection; an index would point at a different sound after any search.
 */
const ROW_H = 28
const OVERSCAN = 8

export type SortKey =
  | 'favorite'
  | 'filename'
  | 'category'
  | 'ucsCategory'
  | 'durationS'
  | 'bpm'
  | 'channels'
  | 'sampleRate'
  | 'filesize'

/**
 * Wire name per sort key.
 *
 * The renderer's keys are the row fields it sorts by in memory (`durationS`,
 * `sampleRate`), but the index whitelists its own column names (`duration`,
 * `sample_rate`). Sending the row field names straight through made `search` fail
 * with `unknown sort column durationS` — and because the failing request kept the
 * same sort in every later call, one click on Length broke category and folder
 * filtering too, until the user happened to sort by something else.
 *
 * `SORT_WIRE` is the single place that translation happens.
 */
export const SORT_WIRE: Record<SortKey, string> = {
  favorite: 'favorite',
  filename: 'filename',
  category: 'category',
  ucsCategory: 'ucs_category',
  durationS: 'duration',
  bpm: 'bpm',
  channels: 'channels',
  sampleRate: 'sample_rate',
  filesize: 'filesize'
}

interface Column {
  key: SortKey
  /** Resolved at render time: the header follows the locale. */
  label: () => string
  width: string
  numeric?: boolean
}

const COLUMNS: Column[] = [
  { key: 'favorite', label: () => '★', width: '32px' },
  { key: 'filename', label: () => t('col.name'), width: 'minmax(180px, 2fr)' },
  { key: 'category', label: () => t('col.category'), width: 'minmax(140px, 1fr)' },
  { key: 'ucsCategory', label: () => t('col.ucsCategory'), width: 'minmax(160px, 1fr)' },
  { key: 'durationS', label: () => t('col.length'), width: '84px', numeric: true },
  { key: 'bpm', label: () => t('col.bpm'), width: '56px', numeric: true },
  { key: 'channels', label: () => t('col.channels'), width: '64px' },
  { key: 'sampleRate', label: () => t('col.rate'), width: '78px', numeric: true },
  { key: 'filesize', label: () => t('col.size'), width: '84px', numeric: true }
]

/**
 * Default direction per column, so the first click on a numeric column is the
 * useful one: nobody sorting by size wants the smallest file first.
 */
export const DEFAULT_DIR: Record<SortKey, 'asc' | 'desc'> = {
  favorite: 'desc',
  filename: 'asc',
  category: 'asc',
  ucsCategory: 'asc',
  durationS: 'desc',
  bpm: 'desc',
  channels: 'desc',
  sampleRate: 'desc',
  filesize: 'desc'
}

/**
 * Where the columns sit, in the user's order.
 *
 * Stored as a plain list of keys rather than an index per column: a key that is
 * no longer in `COLUMNS` simply drops out on load, and a column added later
 * appears at the end instead of vanishing because it was not in a stale list.
 */
const ORDER_KEY = 'mio:column-order'

function loadColumnOrder(): SortKey[] {
  const all = COLUMNS.map((c) => c.key)
  let stored: unknown
  try {
    const raw = localStorage.getItem(ORDER_KEY)
    if (!raw) return all
    stored = JSON.parse(raw)
  } catch {
    return all
  }
  if (!Array.isArray(stored)) return all
  const seen = [...new Set(stored.filter((k): k is SortKey => all.includes(k as SortKey)))]
  return [...seen, ...all.filter((k) => !seen.includes(k))]
}

function formatBytes(bytes: number | null): string {
  if (bytes === null) return '—'
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb.toFixed(0)} KB`
  return `${(kb / 1024).toFixed(1)} MB`
}

/**
 * `128`, or `126.5` for the packs that write half tempos, or an em dash when the
 * name states none. The column is 56px wide, so more than one decimal would clip
 * and more than one decimal is never a real tempo.
 */
function formatBpm(bpm: number | null): string {
  if (bpm === null) return '—'
  return Number.isInteger(bpm) ? String(bpm) : bpm.toFixed(1)
}

function formatRate(rate: number | null): string {
  if (rate === null) return '—'
  return `${(rate / 1000).toFixed(rate % 1000 === 0 ? 0 : 1)}k`
}

export interface RowMenuRequest {
  x: number
  y: number
  targets: SoundRow[]
  count: number
  scope: EditScope
}

export type EditScope =
  | { kind: 'paths'; paths: string[] }
  | {
      kind: 'matching'
      request: SearchRequest
      exclude: string[]
      offset: number
      limit: number
    }

/**
 * One cell of a row, chosen by column key.
 *
 * The cells used to be a fixed sequence of spans under a fixed header. Making the
 * header reorderable means both sides have to be generated from the same column
 * list, or a drag would leave the labels sitting over the wrong values — and that
 * is a class of bug no CSS can catch.
 */
function cellNode(col: SortKey, row: SoundRow) {
  switch (col) {
    case 'favorite':
      // Always a glyph, never an empty span: an empty cell has zero width, so the
      // star could not be clicked until the sound was already a favorite.
      return (
        <span
          class={row.favorite ? 'cell favorite on' : 'cell favorite'}
          title={row.favorite ? t('row.removeFavorite') : t('row.addFavorite')}
        >
          {row.favorite ? '★' : '☆'}
        </span>
      )
    case 'filename':
      return (
        <span
          class="cell name"
          title={row.description ? `${row.path}\n${row.description}` : row.path}
        >
          {row.filename}
          <Show when={row.descriptionCount && row.descriptionCount > 1}>
            <span class="description-count" title={row.description ?? undefined}>
              ×{row.descriptionCount}
            </span>
          </Show>
        </span>
      )
    case 'category':
      return (
        <span class="cell dim" title={row.matchedTerms.join(', ')}>
          {categoryName(row.category)}
          <Show when={row.subcategory}>
            <span class="sub"> / {row.subcategory}</span>
          </Show>
        </span>
      )
    case 'ucsCategory':
      return (
        <span class="cell dim" title={row.ucsCatId ?? undefined}>
          {row.ucsCategory ?? '—'}
          <Show when={row.ucsSubcategory}>
            <span class="sub"> / {row.ucsSubcategory}</span>
          </Show>
        </span>
      )
    case 'durationS':
      return <span class="cell numeric">{row.durationLabel}</span>
    case 'bpm':
      return <span class="cell numeric dim">{formatBpm(row.bpm)}</span>
    case 'channels':
      return <span class="cell dim">{channelLabel(row.channels)}</span>
    case 'sampleRate':
      return <span class="cell numeric dim">{formatRate(row.sampleRate)}</span>
    case 'filesize':
      return <span class="cell numeric dim">{formatBytes(row.filesize)}</span>
    default:
      // A key with no cell is a column the user added to `COLUMNS` and forgot to
      // draw; the header already shows the label, so the row shows a dash rather
      // than misaligning every value after it.
      return <span class="cell dim">—</span>
  }
}

interface Props {
  epoch: number
  selectionReset: number
  request: SearchRequest
  fetchWindow: (offset: number, limit: number) => Promise<SoundRow[]>
  /** The row the keyboard moves from, and the one the dock shows. */
  active: SoundRow | null
  sortKey: SortKey
  sortAsc: boolean
  /** Hide the category column; the value is still filterable from the rail. */
  showCategory: boolean
  /** Total rows matching the filter; also determines the full scroll height. */
  matching: number
  /**
   * A first-page query is in flight: the rows on screen are the previous result
   * and are about to be replaced. Exposed to the DOM as `data-querying` so the
   * CDP checks wait on a real signal instead of guessing sleeps, and shown in the
   * footer so it is not an invisible state.
   */
  querying: boolean
  onSort: (key: SortKey) => void
  onSelectionChange: (count: number, active: SoundRow | null) => void
  /** Clicking a row plays it when auto-play is on. */
  onActivate: (row: SoundRow) => void
  onRowMenu: (request: RowMenuRequest) => void
  onToggleFavorite: (row: SoundRow) => void
  onSelectAll: (count: number) => void
}

const SEGMENT_ROWS = 500
const MAX_SEGMENTS = 24
const MAX_IN_FLIGHT = 8

type Selection =
  | { mode: 'paths'; paths: Set<string> }
  | { mode: 'all'; excluded: Set<string> }
  | { mode: 'range'; lo: number; hi: number; excluded: Set<string> }

export function SoundTable(props: Props) {
  const [scrollTop, setScrollTop] = createSignal(0)
  const [viewportH, setViewportH] = createSignal(600)
  /**
   * Width the body's scrollbar takes. The header is not inside the scrolling
   * element, so without this its column tracks are 8px wider than the rows' and
   * every label sits to the left of the values under it.
   */
  const [scrollbarW, setScrollbarW] = createSignal(0)
  /** Anchor for Shift+click. A path, so a re-query cannot move it to another row. */
  const [anchor, setAnchor] = createSignal<string | null>(null)
  const [segments, setSegments] = createSignal<Map<number, SoundRow[]>>(new Map())
  const [segmentsEpoch, setSegmentsEpoch] = createSignal(props.epoch)
  const segmentUse = new Map<number, number>()
  let useClock = 0
  const [selection, setSelection] = createSignal<Selection>({ mode: 'paths', paths: new Set() })
  const [pendingIndex, setPendingIndex] = createSignal<number | null>(null)
  const [cursorIndex, setCursorIndex] = createSignal<number | null>(null)
  const [anchorIndex, setAnchorIndex] = createSignal<number | null>(null)
  /** The user's column order, as keys. Persisted; see `ORDER_KEY`. */
  const [order, setOrder] = createSignal<SortKey[]>(loadColumnOrder())
  /** The column being dragged, and the one it would land on. */
  const [dragging, setDragging] = createSignal<SortKey | null>(null)
  const [dropTarget, setDropTarget] = createSignal<{ key: SortKey; after: boolean } | null>(null)

  /**
   * Columns actually drawn, which the header and every row must agree on.
   *
   * The favourite star is pinned first: it is one glyph wide and carries no label,
   * so a user dragging it to the far right gets a column they cannot identify. The
   * rest follow `order`.
   */
  const columns = createMemo(() => {
    const shown = COLUMNS.filter((c) => c.key !== 'category' || props.showCategory)
    const rank = order()
    const at = (key: SortKey) => {
      const i = rank.indexOf(key)
      return i < 0 ? rank.length : i
    }
    return shown.slice().sort((a, b) => {
      if (a.key === 'favorite') return -1
      if (b.key === 'favorite') return 1
      return at(a.key) - at(b.key)
    })
  })
  const template = createMemo(() => columns().map((c) => c.width).join(' '))

  /**
   * Move `from` next to `to`.
   *
   * `after` comes from which half of the target header the pointer is over, so a
   * column can be dropped at the far right by aiming at the right half of the last
   * one. Inserting always-before would make that position unreachable.
   */
  const moveColumn = (from: SortKey, to: SortKey, after: boolean) => {
    if (from === to || from === 'favorite' || to === 'favorite') return
    const current = columns()
      .map((c) => c.key)
      .filter((k) => k !== from && k !== 'favorite')
    const at = current.indexOf(to)
    if (at < 0) return
    current.splice(after ? at + 1 : at, 0, from)
    setOrder([...current, ...COLUMNS.map((c) => c.key).filter((k) => !current.includes(k))])
    try {
      localStorage.setItem(ORDER_KEY, JSON.stringify(current))
    } catch {
      // A locked profile costs the arrangement, not the session.
    }
  }

  /** Whether the pointer is in the right half of the header, for `after`. */
  const dropAfter = (event: DragEvent): boolean => {
    const el = event.currentTarget as HTMLElement
    const rect = el.getBoundingClientRect()
    return event.clientX > rect.left + rect.width / 2
  }
  const isDropBefore = (key: SortKey) => {
    const t = dropTarget()
    return t?.key === key && !t.after
  }
  const isDropAfter = (key: SortKey) => {
    const t = dropTarget()
    return t?.key === key && t.after
  }

  let body: HTMLDivElement | undefined

  const rowAt = (index: number): SoundRow | undefined => {
    if (index < 0 || index >= props.matching) return undefined
    const segment = segments().get(Math.floor(index / SEGMENT_ROWS))
    return segment?.[index % SEGMENT_ROWS]
  }

  const pathIndex = createMemo(() => {
    const index = new Map<string, number>()
    for (const [segment, rows] of segments()) {
      rows.forEach((row, offset) => index.set(row.path, segment * SEGMENT_ROWS + offset))
    }
    return index
  })

  const indexOf = (path: string | null): number =>
    path === null ? -1 : pathIndex().get(path) ?? -1

  const activateAt = (row: SoundRow, index: number) => {
    const neighbors = [-2, -1, 1, 2]
      .map((offset) => rowAt(index + offset)?.path)
      .filter((path): path is string => path !== undefined)
    if (neighbors.length) void api.prefetchNeighbors(neighbors)
    props.onActivate(row)
  }

  const selectionCount = () => {
    const current = selection()
    if (current.mode === 'paths') return current.paths.size
    if (current.mode === 'all') return Math.max(0, props.matching - current.excluded.size)
    return Math.max(0, current.hi - current.lo + 1 - current.excluded.size)
  }

  const isSelected = (row: SoundRow, index: number): boolean => {
    const current = selection()
    if (current.mode === 'paths') return current.paths.has(row.path)
    if (current.mode === 'all') return !current.excluded.has(row.path)
    return index >= current.lo && index <= current.hi && !current.excluded.has(row.path)
  }

  const loadedRows = createMemo(() => {
    const out: { row: SoundRow; index: number }[] = []
    for (const [segment, rows] of segments()) {
      rows.forEach((row, offset) => out.push({ row, index: segment * SEGMENT_ROWS + offset }))
    }
    return out
  })

  const fetchSegment = (segment: number, requestedEpoch: number): void => {
    if (segment < 0 || segment * SEGMENT_ROWS >= props.matching) return
    if (segmentsEpoch() === requestedEpoch && segments().has(segment)) return
    const key = `${requestedEpoch}:${segment}`
    if (inFlight.has(key) || backlog.has(key)) return
    // The in-flight cap is not a "drop on the floor" gate: a burst of programmatic
    // jumps (the scroll-heap check) can occupy every slot, and a skipped segment
    // would never be retried — the viewport would sit on skeletons until the user
    // scrolled again. Park it in `backlog` instead; when a slot frees up the
    // backlog drains oldest-first.
    if (inFlight.size >= MAX_IN_FLIGHT) {
      backlog.set(key, { segment, epoch: requestedEpoch })
      return
    }
    let task!: Promise<void>
    task = (async () => {
      try {
        const rows = await props.fetchWindow(segment * SEGMENT_ROWS, SEGMENT_ROWS)
        if (requestedEpoch !== props.epoch) return
        if (segmentsEpoch() !== requestedEpoch) {
          setSegments(new Map())
          segmentUse.clear()
          setSegmentsEpoch(requestedEpoch)
        }
        segmentUse.set(segment, ++useClock)
        setSegments((previous) => {
          const next = new Map(previous)
          next.set(segment, rows)
          while (next.size > MAX_SEGMENTS) {
            let oldestSegment: number | undefined
            let oldestUse = Number.POSITIVE_INFINITY
            for (const cached of next.keys()) {
              const used = segmentUse.get(cached) ?? 0
              if (used < oldestUse) {
                oldestUse = used
                oldestSegment = cached
              }
            }
            if (oldestSegment === undefined) break
            next.delete(oldestSegment)
            segmentUse.delete(oldestSegment)
          }
          return next
        })
      } catch {
        // A failed window can be retried the next time it enters the viewport.
      } finally {
        if (inFlight.get(key) === task) inFlight.delete(key)
        drainBacklog()
      }
    })()
    inFlight.set(key, task)
  }

  /**
   * Segments that hit the in-flight cap and wait for a slot to free up.
   *
   * Trimmed to the viewport on every scroll: a scrollbar drag across half a
   * 280k library would otherwise park hundreds of segment requests that are
   * never shown again, each of which later runs a real query when a slot frees.
   */
  const backlog = new Map<string, { segment: number; epoch: number }>()
  const trimBacklog = (firstSegment: number, lastSegment: number) => {
    for (const [key, item] of backlog) {
      if (item.segment < firstSegment - 1 || item.segment > lastSegment + 1) backlog.delete(key)
    }
  }
  const drainBacklog = () => {
    for (const [key, item] of [...backlog]) {
      const { segment, epoch } = item
      if (epoch !== props.epoch) {
        backlog.delete(key)
        continue
      }
      if (segmentsEpoch() === epoch && segments().has(segment)) {
        backlog.delete(key)
        continue
      }
      if (inFlight.has(key)) continue
      if (inFlight.size >= MAX_IN_FLIGHT) break
      backlog.delete(key)
      fetchSegment(segment, epoch)
    }
  }

  const inFlight = new Map<string, Promise<void>>()

  const range = createMemo(() => {
    const total = props.matching
    const first = Math.min(total, Math.max(0, Math.floor(scrollTop() / ROW_H) - OVERSCAN))
    const visible = Math.ceil(viewportH() / ROW_H) + OVERSCAN * 2
    const last = Math.max(first, Math.min(total, first + visible))
    return { first, last }
  })

  let lastFilterKey: string | null = null
  let lastSortKey: string | null = null
  let lastSelectionReset = props.selectionReset

  createEffect(() => {
    const reset = props.selectionReset
    if (reset !== lastSelectionReset) {
      lastSelectionReset = reset
      setSelection({ mode: 'paths', paths: new Set() })
      setAnchor(null)
      setAnchorIndex(null)
      setCursorIndex(null)
      setPendingIndex(null)
      props.onSelectionChange(0, null)
    }
  })

  const visibleIndices = createMemo(() => {
    const { first, last } = range()
    return Array.from({ length: last - first }, (_, offset) => first + offset)
  })

  createEffect(() => {
    const epoch = props.epoch
    if (!props.querying && props.matching === 0 && segmentsEpoch() !== epoch) {
      setSegmentsEpoch(epoch)
      backlog.clear()
      segmentUse.clear()
      setSegments(new Map())
    }
  })

  createEffect(() => {
    const epoch = props.epoch
    for (const [key, queued] of backlog) {
      if (queued.epoch !== epoch) backlog.delete(key)
    }
    if (props.querying) return
    const { first, last } = range()
    const cached = segments()
    const firstSegment = Math.floor(first / SEGMENT_ROWS)
    const lastSegment = Math.floor(Math.max(first, last - 1) / SEGMENT_ROWS)
    // Drop queue entries the viewport has left before they can spend a slot.
    trimBacklog(firstSegment, lastSegment)
    for (let segment = firstSegment; segment <= lastSegment; segment++) {
      if (cached.has(segment)) segmentUse.set(segment, ++useClock)
    }
    for (let segment = Math.max(0, firstSegment - 1); segment <= lastSegment + 1; segment++) {
      void fetchSegment(segment, epoch)
    }
  })

  createEffect(() => {
    const epoch = props.epoch
    const pending = pendingIndex()
    if (pending === null) return
    if (segmentsEpoch() !== epoch) return
    const row = rowAt(pending)
    if (row) {
      setPendingIndex(null)
      setAnchor(row.path)
      setAnchorIndex(pending)
      setCursorIndex(pending)
      setSelection({ mode: 'paths', paths: new Set([row.path]) })
      props.onSelectionChange(1, row)
      activateAt(row, pending)
    } else if (pending >= props.matching || epoch !== props.epoch) {
      setPendingIndex(null)
    }
  })

  createEffect(() => {
    const filterKey = JSON.stringify({
      query: props.request.query,
      categories: props.request.categories,
      subcategories: props.request.subcategories,
      folder: props.request.folder,
      favoriteOnly: props.request.favoriteOnly
    })
    const sortKey = `${props.request.sortBy ?? ''}:${props.request.sortDir ?? ''}`
    if (lastFilterKey !== null && filterKey !== lastFilterKey) {
      setSelection({ mode: 'paths', paths: new Set() })
      setAnchor(null)
      setAnchorIndex(null)
      setCursorIndex(null)
      props.onSelectionChange(0, null)
      if (body) body.scrollTop = 0
      setScrollTop(0)
    } else if (lastSortKey !== null && sortKey !== lastSortKey) {
      setAnchorIndex(null)
      setCursorIndex(null)
      if (selection().mode === 'range') {
        setSelection({ mode: 'paths', paths: new Set() })
        setAnchor(null)
        props.onSelectionChange(0, null)
      }
      if (body) body.scrollTop = 0
      setScrollTop(0)
    }
    lastFilterKey = filterKey
    lastSortKey = sortKey
  })

  const scrollToRow = (index: number) => {
    const el = body
    if (!el) return
    const top = index * ROW_H
    const bottom = top + ROW_H
    if (top < el.scrollTop) el.scrollTop = top
    else if (bottom > el.scrollTop + el.clientHeight) el.scrollTop = bottom - el.clientHeight
  }

  const reportSelection = (next: Selection, active: SoundRow | null) => {
    setSelection(next)
    props.onSelectionChange(selectionCount(), active)
  }

  /** Click behaviour: plain replaces, Ctrl toggles/excludes, Shift selects an index range. */
  const handleRowClick = (row: SoundRow, index: number, event: MouseEvent) => {
    if (event.shiftKey) {
      const from = anchorIndex() ?? indexOf(anchor() ?? props.active?.path ?? null)
      if (from >= 0) {
        const [lo, hi] = from <= index ? [from, index] : [index, from]
        reportSelection({ mode: 'range', lo, hi, excluded: new Set() }, row)
        setAnchorIndex(from)
        setCursorIndex(index)
        return
      }
    }
    if (event.ctrlKey || event.metaKey) {
      const current = selection()
      if (current.mode === 'all') {
        const excluded = new Set(current.excluded)
        excluded.has(row.path) ? excluded.delete(row.path) : excluded.add(row.path)
        setAnchor(row.path)
        setAnchorIndex(index)
        setCursorIndex(index)
        reportSelection({ mode: 'all', excluded }, row)
        return
      }
      if (current.mode === 'range' && index >= current.lo && index <= current.hi) {
        const excluded = new Set(current.excluded)
        excluded.has(row.path) ? excluded.delete(row.path) : excluded.add(row.path)
        setAnchor(row.path)
        setAnchorIndex(index)
        setCursorIndex(index)
        reportSelection({ ...current, excluded }, row)
        return
      }
      const next = new Set(current.mode === 'paths' ? current.paths : [])
      if (next.has(row.path)) next.delete(row.path)
      else next.add(row.path)
      setAnchor(row.path)
      setAnchorIndex(index)
      setCursorIndex(index)
      reportSelection({ mode: 'paths', paths: next }, row)
      return
    }
    setAnchor(row.path)
    setAnchorIndex(index)
    setCursorIndex(index)
    reportSelection({ mode: 'paths', paths: new Set([row.path]) }, row)
    activateAt(row, index)
  }

  /**
   * Keyboard, per the reference implementation: arrows move and audition,
   * Home/End jump, PageUp/PageDown move by a screen, Ctrl+A selects the filter.
   *
   * Bound on the window rather than on the table so it works whether the focus is
   * on a row or on the table body, and skipped while a text field has focus so
   * typing "rain" does not audition every letter typed.
   */
  const onKeyDown = (event: KeyboardEvent) => {
    const el = document.activeElement as HTMLElement | null
    // iX's own inputs render a native input inside a shadow root; `activeElement`
    // is then the host, so the tag name is checked against the host's too.
    const tag = el?.tagName ?? ''
    const inField =
      tag === 'INPUT' ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      tag.startsWith('IX-') ||
      el?.isContentEditable === true

    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a' && !inField) {
      event.preventDefault()
      setSelection({ mode: 'all', excluded: new Set() })
      props.onSelectionChange(props.matching, null)
      props.onSelectAll(props.matching)
      return
    }

    if (inField || event.ctrlKey || event.metaKey || event.altKey) return
    if (props.matching === 0) return

    const current = cursorIndex() ?? indexOf(props.active?.path ?? null)
    const move = (next: number) => {
      setCursorIndex(next)
      const row = rowAt(next)
      scrollToRow(next)
      if (!row) {
        setPendingIndex(next)
        return
      }
      setPendingIndex(null)
      setAnchor(row.path)
      setAnchorIndex(next)
      reportSelection({ mode: 'paths', paths: new Set([row.path]) }, row)
      activateAt(row, next)
    }

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        move(current < 0 ? 0 : Math.min(current + 1, props.matching - 1))
        break
      case 'ArrowUp':
        event.preventDefault()
        move(current < 0 ? 0 : Math.max(current - 1, 0))
        break
      case 'PageDown':
        event.preventDefault()
        move(Math.min(props.matching - 1, (current < 0 ? 0 : current) + 10))
        break
      case 'PageUp':
        event.preventDefault()
        move(Math.max(0, (current < 0 ? 0 : current) - 10))
        break
      case 'Home':
        event.preventDefault()
        move(0)
        break
      case 'End':
        event.preventDefault()
        move(props.matching - 1)
        break
      default:
        break
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  const selectedRows = () => loadedRows().filter(({ row, index }) => isSelected(row, index))

  const editScope = (): EditScope => {
    const current = selection()
    if (current.mode === 'paths') return { kind: 'paths', paths: [...current.paths] }
    if (current.mode === 'all') {
      return {
        kind: 'matching',
        request: { ...props.request },
        exclude: [...current.excluded],
        offset: 0,
        limit: 0xffff_ffff
      }
    }
    return {
      kind: 'matching',
      request: { ...props.request },
      exclude: [...current.excluded],
      offset: current.lo,
      limit: current.hi - current.lo + 1
    }
  }

  const rowFromEvent = (event: Event): { row: SoundRow; index: number; element: HTMLElement } | null => {
    const target = event.target
    if (!(target instanceof Element)) return null
    const element = target.closest<HTMLElement>('.row[data-index]')
    if (!element) return null
    const index = Number(element.dataset.index)
    const row = rowAt(index)
    return row ? { row, index, element } : null
  }

  const onBodyClick = (event: MouseEvent) => {
    const found = rowFromEvent(event)
    if (!found) return
    const target = event.target
    if (target instanceof Element && target.closest('.cell.favorite')) {
      props.onToggleFavorite(found.row)
      return
    }
    handleRowClick(found.row, found.index, event)
  }

  const onBodyDoubleClick = (event: MouseEvent) => {
    const found = rowFromEvent(event)
    if (found) activateAt(found.row, found.index)
  }

  const onBodyContextMenu = (event: MouseEvent) => {
    const found = rowFromEvent(event)
    if (!found) return
    event.preventDefault()
    if (!isSelected(found.row, found.index)) {
      setAnchor(found.row.path)
      setAnchorIndex(found.index)
      setCursorIndex(found.index)
      reportSelection({ mode: 'paths', paths: new Set([found.row.path]) }, found.row)
      props.onRowMenu({
        x: event.clientX,
        y: event.clientY,
        targets: [found.row],
        count: 1,
        scope: { kind: 'paths', paths: [found.row.path] }
      })
      return
    }
    const targets = selectedRows().map(({ row }) => row)
    props.onRowMenu({
      x: event.clientX,
      y: event.clientY,
      targets,
      count: selectionCount(),
      scope: editScope()
    })
  }

  const onBodyDragStart = (event: DragEvent) => {
    const found = rowFromEvent(event)
    if (!found) return
    event.preventDefault()
    const targets = isSelected(found.row, found.index)
      ? selectedRows().map(({ row }) => row.path)
      : [found.row.path]
    void startDrag(targets)
  }

  onMount(() => {
    const el = body
    if (!el) return
    el.addEventListener('click', onBodyClick)
    el.addEventListener('dblclick', onBodyDoubleClick)
    el.addEventListener('contextmenu', onBodyContextMenu)
    el.addEventListener('dragstart', onBodyDragStart)
    onCleanup(() => {
      el.removeEventListener('click', onBodyClick)
      el.removeEventListener('dblclick', onBodyDoubleClick)
      el.removeEventListener('contextmenu', onBodyContextMenu)
      el.removeEventListener('dragstart', onBodyDragStart)
    })
  })

  return (
    <div
      class="table"
      data-querying={props.querying ? 'true' : 'false'}
      data-epoch={props.epoch}
      data-segments-epoch={segmentsEpoch()}
    >
      <div
        class="table-head"
        style={{
          'grid-template-columns': template(),
          width: `calc(100% - ${scrollbarW()}px)`
        }}
      >
        <For each={columns()}>
          {(col) => (
            <button
              class={col.numeric ? 'head-cell numeric' : 'head-cell'}
              data-sort-key={col.key}
              data-drop={isDropBefore(col.key) ? 'before' : isDropAfter(col.key) ? 'after' : undefined}
              draggable={col.key !== 'favorite'}
              onClick={() => props.onSort(col.key)}
              onDragStart={(e) => {
                setDragging(col.key)
                // The drag stays inside the header, but a payload is what makes
                // WebView2 start it at all; with an empty DataTransfer no `dragover`
                // fires on the other headers and nothing can be dropped.
                e.dataTransfer?.setData('text/plain', col.key)
                if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
              }}
              onDragOver={(e) => {
                const from = dragging()
                if (!from || from === col.key || col.key === 'favorite') return
                e.preventDefault()
                if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
                setDropTarget({ key: col.key, after: dropAfter(e) })
              }}
              onDragLeave={() => {
                if (dropTarget()?.key === col.key) setDropTarget(null)
              }}
              onDrop={(e) => {
                e.preventDefault()
                const from = dragging()
                setDropTarget(null)
                setDragging(null)
                if (from) moveColumn(from, col.key, dropAfter(e))
              }}
              onDragEnd={() => {
                setDragging(null)
                setDropTarget(null)
              }}
              title={
                col.key === 'favorite'
                  ? t('col.sortBy', { column: col.label() })
                  : `${t('col.sortBy', { column: col.label() })} · ${t('col.reorder')}`
              }
            >
              {col.label()}
              <Show when={props.sortKey === col.key}>
                <span class="sort-arrow">{props.sortAsc ? '▲' : '▼'}</span>
              </Show>
            </button>
          )}
        </For>
      </div>

      <div
        class="table-body"
        ref={(el) => {
          body = el
          const measure = () => {
            setViewportH(el.clientHeight)
            // `offsetWidth - clientWidth` is the scrollbar, measured rather than
            // assumed: this is 8px in the stylesheet but 0 when the OS overlays
            // scrollbars, and hardcoding either breaks the alignment.
            setScrollbarW(el.offsetWidth - el.clientWidth)
          }
          const observer = new ResizeObserver(measure)
          observer.observe(el)
          measure()
          // Without this the observer holds the body element after the table is
          // gone, which pins the detached subtree against collection.
          onCleanup(() => observer.disconnect())
        }}
        onScroll={(e) => {
          const el = e.currentTarget
          setScrollTop(el.scrollTop)
        }}
      >
        <div role="list" style={{ height: `${props.matching * ROW_H}px`, position: 'relative' }}>
          <For each={visibleIndices()}>
            {(absoluteIndex) => {
              const row = () => rowAt(absoluteIndex)
              return (
                <Show
                  when={row()}
                  fallback={
                    <div
                      class="row skeleton"
                      aria-hidden="true"
                      aria-setsize={props.matching}
                      aria-posinset={absoluteIndex + 1}
                      style={{
                        position: 'absolute',
                        top: `${absoluteIndex * ROW_H}px`,
                        height: `${ROW_H}px`,
                        'grid-template-columns': template()
                      }}
                    />
                  }
                >
                  {(loaded) => {
                    const currentRow = loaded()
                    const isActive = () => props.active?.path === currentRow.path
                    const isRowSelected = () => isSelected(currentRow, absoluteIndex)
                    return (
                      <div
                        class={`row${isRowSelected() ? ' selected' : ''}${isActive() ? ' active' : ''}`}
                        role="listitem"
                        data-path={currentRow.path}
                        data-index={absoluteIndex}
                        aria-setsize={props.matching}
                        aria-posinset={absoluteIndex + 1}
                        style={{
                          position: 'absolute',
                          top: `${absoluteIndex * ROW_H}px`,
                          height: `${ROW_H}px`,
                          'grid-template-columns': template()
                        }}
                        draggable={true}
                      >
                        {columns().map((col) => cellNode(col.key, currentRow))}
                      </div>
                    )
                  }}
                </Show>
              )
            }}
          </For>
        </div>
      </div>

      <Show when={props.matching === 0}>
        <div class="table-empty">{t('table.noMatch')}</div>
      </Show>

      <Show when={props.querying}>
        <div class="table-foot">
          <span class="foot-querying">{t('table.searching')}</span>
        </div>
      </Show>
    </div>
  )
}
