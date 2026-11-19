import { For, Show, createMemo, createSignal, onCleanup, onMount } from 'solid-js'
import { categoryName, channelLabel, t } from './i18n'
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
const ROW_H = 28
const OVERSCAN = 8

export type SortKey =
  | 'favorite'
  | 'filename'
  | 'category'
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
}

/**
 * One cell of a row, chosen by column key.
 *
 * The cells used to be a fixed sequence of spans under a fixed header. Making the
 * header reorderable means both sides have to be generated from the same column
 * list, or a drag would leave the labels sitting over the wrong values — and that
 * is a class of bug no CSS can catch.
 */
function Cell(props: {
  col: SortKey
  row: SoundRow
  onToggleFavorite: (row: SoundRow) => void
}) {
  switch (props.col) {
    case 'favorite':
      // Always a glyph, never an empty span: an empty cell has zero width, so the
      // star could not be clicked until the sound was already a favorite.
      return (
        <span
          class={props.row.favorite ? 'cell favorite on' : 'cell favorite'}
          onClick={(e) => {
            e.stopPropagation()
            props.onToggleFavorite(props.row)
          }}
          title={props.row.favorite ? t('row.removeFavorite') : t('row.addFavorite')}
        >
          {props.row.favorite ? '★' : '☆'}
        </span>
      )
    case 'filename':
      return (
        <span class="cell name" title={props.row.path}>
          {props.row.filename}
        </span>
      )
    case 'category':
      return (
        <span class="cell dim" title={props.row.matchedTerms.join(', ')}>
          {categoryName(props.row.category)}
          <Show when={props.row.subcategory}>
            <span class="sub"> / {props.row.subcategory}</span>
          </Show>
        </span>
      )
    case 'durationS':
      return <span class="cell numeric">{props.row.durationLabel}</span>
    case 'bpm':
      return <span class="cell numeric dim">{formatBpm(props.row.bpm)}</span>
    case 'channels':
      return <span class="cell dim">{channelLabel(props.row.channels)}</span>
    case 'sampleRate':
      return <span class="cell numeric dim">{formatRate(props.row.sampleRate)}</span>
    case 'filesize':
      return <span class="cell numeric dim">{formatBytes(props.row.filesize)}</span>
    default:
      // A key with no cell is a column the user added to `COLUMNS` and forgot to
      // draw; the header already shows the label, so the row shows a dash rather
      // than misaligning every value after it.
      return <span class="cell dim">—</span>
  }
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
   * element, so without this its column tracks are 8px wider than the rows' and
   * every label sits to the left of the values under it.
   */
  const [scrollbarW, setScrollbarW] = createSignal(0)
  /** Anchor for Shift+click. A path, so a re-query cannot move it to another row. */
  const [anchor, setAnchor] = createSignal<string | null>(null)
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
   *
   * `draggable` must be bound as a boolean. A bare `draggable` attribute renders
   * as `draggable=""`, which is not a valid value for this enumerated attribute:
   * the element falls back to `auto`, `row.draggable` reads `false`, and the
   * browser never starts a drag at all — so this handler never ran and dragging
   * a row into a DAW silently did nothing.
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
                  draggable={true}
                  onDragStart={(e) => onRowDragStart(row, e)}
                >
                  {/* One cell per column, chosen by key rather than written in a
                      fixed sequence: the header is reorderable now, so the labels
                      and the values come from the same list — otherwise a drag
                      would put "Length" over a sample rate. */}
                  <For each={columns()}>
                    {(col) => (
                      <Cell col={col.key} row={row} onToggleFavorite={props.onToggleFavorite} />
                    )}
                  </For>
                </div>
              )
            }}
          </For>
        </div>
      </div>

      <Show when={props.rows.length === 0}>
        <div class="table-empty">{t('table.noMatch')}</div>
      </Show>

      {/*
        The footer says how much of the filter is loaded. With infinite scroll the
        number creeps up as you scroll; without it, this is the only place that
        tells you the list is not the whole library.
      */}
      <Show when={props.rows.length > 0}>
        <div class="table-foot">
          <Show
            when={props.querying}
            fallback={
              <>
                {t('table.count', {
                  shown: props.rows.length.toLocaleString(),
                  total: props.matching.toLocaleString()
                })}
                <Show when={props.hasMore}>
                  <span> · {t('table.scrollForMore')}</span>
                </Show>
              </>
            }
          >
            <span class="foot-querying">{t('table.searching')}</span>
          </Show>
        </div>
      </Show>
    </div>
  )
}
