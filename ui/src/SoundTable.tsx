import { For, Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { startDrag, type SoundRow } from './ipc'

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
const ROW_H = 26
const OVERSCAN = 8

export type SortKey =
  | 'favorite'
  | 'filename'
  | 'category'
  | 'durationS'
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
  durationS: 'duration',
  channels: 'channels',
  sampleRate: 'sample_rate',
  filesize: 'filesize'
}

interface Column {
  key: SortKey
  label: string
  width: string
  numeric?: boolean
}

const COLUMNS: Column[] = [
  { key: 'favorite', label: '★', width: '30px' },
  { key: 'filename', label: 'Name', width: 'minmax(180px, 2fr)' },
  { key: 'category', label: 'Category', width: 'minmax(140px, 1fr)' },
  { key: 'durationS', label: 'Length', width: '82px', numeric: true },
  { key: 'channels', label: 'Ch', width: '62px' },
  { key: 'sampleRate', label: 'Rate', width: '76px', numeric: true },
  { key: 'filesize', label: 'Size', width: '82px', numeric: true }
]

/**
 * Default direction per column, so the first click on a numeric column is the
 * useful one: nobody sorting by size wants the smallest file first.
 */
export const DEFAULT_DIR: Record<SortKey, 'asc' | 'desc'> = {
  favorite: 'desc',
  filename: 'asc',
  category: 'asc',
  durationS: 'desc',
  channels: 'desc',
  sampleRate: 'desc',
  filesize: 'desc'
}

function formatBytes(bytes: number | null): string {
  if (bytes === null) return '—'
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb.toFixed(0)} KB`
  return `${(kb / 1024).toFixed(1)} MB`
}

function formatRate(rate: number | null): string {
  if (rate === null) return '—'
  return `${(rate / 1000).toFixed(rate % 1000 === 0 ? 0 : 1)}k`
}

export interface RowMenuRequest {
  x: number
  y: number
  targets: SoundRow[]
}

interface Props {
  rows: SoundRow[]
  /** Paths currently selected. Owned by the app so the dock and menu can read it. */
  selection: string[]
  /** The row the keyboard moves from, and the one the dock shows. */
  active: SoundRow | null
  sortKey: SortKey
  sortAsc: boolean
  /** Hide the category column; the value is still filterable from the rail. */
  showCategory: boolean
  /** Total rows the filter matches, for the "showing N of M" footer. */
  matching: number
  /** Whether another page can still be fetched. */
  hasMore: boolean
  /**
   * A first-page query is in flight: the rows on screen are the previous result
   * and are about to be replaced. Exposed to the DOM as `data-querying` so the
   * CDP checks wait on a real signal instead of guessing sleeps, and shown in the
   * footer so it is not an invisible state.
   */
  querying: boolean
  onSort: (key: SortKey) => void
  onSelectionChange: (paths: string[], active: SoundRow | null) => void
  /** Clicking a row plays it when auto-play is on. */
  onActivate: (row: SoundRow) => void
  onRowMenu: (request: RowMenuRequest) => void
  onToggleFavorite: (row: SoundRow) => void
  /** Ctrl+A: select every row the current filter matches, not just the page. */
  onSelectAll: () => void
  /** Infinite scroll: called when the viewport nears the end of what is loaded. */
  onLoadMore: () => void
}

export function SoundTable(props: Props) {
  const [scrollTop, setScrollTop] = createSignal(0)
  const [viewportH, setViewportH] = createSignal(600)
  /**
   * Width the body's scrollbar takes. The header is not inside the scrolling
   * element, so without this its column tracks are 25px wider than the rows' and
   * every label sits to the left of the values under it.
   */
  const [scrollbarW, setScrollbarW] = createSignal(0)
  /** Anchor for Shift+click. A path, so a re-query cannot move it to another row. */
  const [anchor, setAnchor] = createSignal<string | null>(null)

  /** Columns actually drawn, which the header and every row must agree on. */
  const columns = createMemo(() =>
    COLUMNS.filter((c) => c.key !== 'category' || props.showCategory)
  )
  const template = createMemo(() => columns().map((c) => c.width).join(' '))

  let body: HTMLDivElement | undefined

  const sorted = createMemo(() => {
    const key = props.sortKey
    const dir = props.sortAsc ? 1 : -1
    const copy = props.rows.slice()
    copy.sort((a, b) => {
      const av = a[key]
      const bv = b[key]
      if (av === bv) return a.filename.localeCompare(b.filename) * dir
      // NULLs last in both directions: an unprobed file must not jump to the top
      // just because the user sorted descending.
      if (av === null || av === undefined) return 1
      if (bv === null || bv === undefined) return -1
      if (typeof av === 'number' && typeof bv === 'number') return (av - bv) * dir
      return String(av).localeCompare(String(bv)) * dir
    })
    return copy
  })

  const selectedSet = createMemo(() => new Set(props.selection))

  const range = createMemo(() => {
    const total = sorted().length
    const first = Math.max(0, Math.floor(scrollTop() / ROW_H) - OVERSCAN)
    const visible = Math.ceil(viewportH() / ROW_H) + OVERSCAN * 2
    const last = Math.min(total, first + visible)
    return { first, last }
  })

  const indexOf = (path: string | null): number =>
    path === null ? -1 : sorted().findIndex((r) => r.path === path)

  const scrollToRow = (index: number) => {
    const el = body
    if (!el) return
    const top = index * ROW_H
    const bottom = top + ROW_H
    if (top < el.scrollTop) el.scrollTop = top
    else if (bottom > el.scrollTop + el.clientHeight) el.scrollTop = bottom - el.clientHeight
  }

  /** Click behaviour: plain replaces, Ctrl toggles, Shift extends from the anchor. */
  const handleRowClick = (row: SoundRow, event: MouseEvent) => {
    if (event.shiftKey) {
      const from = indexOf(anchor() ?? props.active?.path ?? null)
      const to = indexOf(row.path)
      if (from >= 0 && to >= 0) {
        const [lo, hi] = from <= to ? [from, to] : [to, from]
        const paths = sorted()
          .slice(lo, hi + 1)
          .map((r) => r.path)
        props.onSelectionChange(paths, row)
        return
      }
    }
    if (event.ctrlKey || event.metaKey) {
      const next = new Set(props.selection)
      if (next.has(row.path)) next.delete(row.path)
      else next.add(row.path)
      setAnchor(row.path)
      props.onSelectionChange([...next], row)
      return
    }
    setAnchor(row.path)
    props.onSelectionChange([row.path], row)
    props.onActivate(row)
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
    const inField =
      el?.tagName === 'INPUT' ||
      el?.tagName === 'TEXTAREA' ||
      el?.tagName === 'SELECT' ||
      el?.isContentEditable === true

    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a' && !inField) {
      event.preventDefault()
      props.onSelectAll()
      return
    }

    if (inField || event.ctrlKey || event.metaKey || event.altKey) return
    if (props.rows.length === 0) return

    const current = indexOf(props.active?.path ?? null)
    const move = (next: number) => {
      const row = sorted()[next]
      if (!row) return
      setAnchor(row.path)
      props.onSelectionChange([row.path], row)
      props.onActivate(row)
      scrollToRow(next)
    }

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        move(current < 0 ? 0 : Math.min(current + 1, sorted().length - 1))
        break
      case 'ArrowUp':
        event.preventDefault()
        move(current < 0 ? 0 : Math.max(current - 1, 0))
        break
      case 'PageDown':
        event.preventDefault()
        move(Math.min(sorted().length - 1, (current < 0 ? 0 : current) + 10))
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
        move(sorted().length - 1)
        break
      default:
        break
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  /**
   * Drag out to a DAW. `preventDefault` on dragstart is deliberate: the Tauri
   * drag plugin starts a native OLE drag itself, and letting WebView2 also start
   * a DOM drag would hand the OS two competing drag sources.
   */
  const onRowDragStart = (row: SoundRow, event: DragEvent) => {
    event.preventDefault()
    const paths = selectedSet().has(row.path) ? props.selection : [row.path]
    void startDrag(paths)
  }

  return (
    <div class="table" data-querying={props.querying ? 'true' : 'false'}>
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
              onClick={() => props.onSort(col.key)}
              title={`Sort by ${col.label}`}
            >
              {col.label}
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
            // assumed: this is 10px in the stylesheet but 0 when the OS overlays
            // scrollbars, and hardcoding either breaks the alignment.
            setScrollbarW(el.offsetWidth - el.clientWidth)
          }
          const observer = new ResizeObserver(measure)
          observer.observe(el)
          measure()
        }}
        onScroll={(e) => {
          const el = e.currentTarget
          setScrollTop(el.scrollTop)
          // Infinite scroll: ask for the next page once the viewport is within
          // two screens of the end. The threshold is generous on purpose — a
          // search answers in ~2ms, so there is no reason to wait for the user to
          // hit the bottom and see a gap.
          if (!props.hasMore) return
          const remaining = el.scrollHeight - el.scrollTop - el.clientHeight
          if (remaining < el.clientHeight * 2) props.onLoadMore()
        }}
      >
        <div style={{ height: `${sorted().length * ROW_H}px`, position: 'relative' }}>
          <For each={sorted().slice(range().first, range().last)}>
            {(row, index) => {
              const isActive = () => props.active?.path === row.path
              const isSelected = () => selectedSet().has(row.path)
              return (
                <div
                  class={`row${isSelected() ? ' selected' : ''}${isActive() ? ' active' : ''}`}
                  data-path={row.path}
                  style={{
                    position: 'absolute',
                    top: `${(range().first + index()) * ROW_H}px`,
                    height: `${ROW_H}px`,
                    'grid-template-columns': template()
                  }}
                  onClick={(e) => handleRowClick(row, e)}
                  onDblClick={() => props.onActivate(row)}
                  onContextMenu={(e) => {
                    e.preventDefault()
                    // Right-clicking outside the selection selects that row first,
                    // so the menu never acts on sounds the user cannot see.
                    if (!selectedSet().has(row.path)) {
                      setAnchor(row.path)
                      props.onSelectionChange([row.path], row)
                      props.onRowMenu({ x: e.clientX, y: e.clientY, targets: [row] })
                      return
                    }
                    const targets = sorted().filter((r) => selectedSet().has(r.path))
                    props.onRowMenu({ x: e.clientX, y: e.clientY, targets })
                  }}
                  draggable
                  onDragStart={(e) => onRowDragStart(row, e)}
                >
                  <span
                    class={row.favorite ? 'cell favorite on' : 'cell favorite'}
                    onClick={(e) => {
                      e.stopPropagation()
                      props.onToggleFavorite(row)
                    }}
                    title={row.favorite ? 'Remove from favorites' : 'Add to favorites'}
                  >
                    {/*
                      Always a glyph, never an empty span: an empty cell has zero
                      width, so the star could not be clicked until the sound was
                      already a favorite — which made starring impossible from the
                      row.
                    */}
                    {row.favorite ? '★' : '☆'}
                  </span>
                  <span class="cell name" title={row.path}>
                    {row.filename}
                  </span>
                  <Show when={props.showCategory}>
                    <span class="cell dim" title={row.matchedTerms.join(', ')}>
                      {row.category}
                      <Show when={row.subcategory}>
                        <span class="sub"> / {row.subcategory}</span>
                      </Show>
                    </span>
                  </Show>
                  <span class="cell numeric">{row.durationLabel}</span>
                  <span class="cell dim">{row.channelLabel}</span>
                  <span class="cell numeric dim">{formatRate(row.sampleRate)}</span>
                  <span class="cell numeric dim">{formatBytes(row.filesize)}</span>
                </div>
              )
            }}
          </For>
        </div>
      </div>

      <Show when={props.rows.length === 0}>
        <div class="table-empty">No sounds match.</div>
      </Show>

      {/*
        The footer says how much of the filter is loaded. With infinite scroll the
        number creeps up as you scroll; without it, this is the only place that
        tells you the list is not the whole library.
      */}
      <Show when={props.rows.length > 0}>
        <div class="table-foot dim">
          <Show when={props.querying} fallback={
            <>
              {props.rows.length.toLocaleString()} of {props.matching.toLocaleString()}
              <Show when={props.hasMore}>
                <span> · scroll for more</span>
              </Show>
            </>
          }>
            <span class="foot-querying">searching…</span>
          </Show>
        </div>
      </Show>
    </div>
  )
}
