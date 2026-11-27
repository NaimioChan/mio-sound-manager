import { createSignal } from 'solid-js'
import { t } from './i18n'

/**
 * Column definitions, order, widths and visibility — one owner.
 *
 * These used to live inside `SoundTable`, which meant the settings dialog and the
 * table's own header menu each had to reach into the table to change a column.
 * Moving them into a module keeps the header, the skeleton rows and the real rows
 * reading the same template, and gives the settings dialog a way to render the
 * list without a second copy of the column table.
 *
 * Signals only, no memos: a module-level `createMemo` runs outside a Solid owner,
 * never disposes, and warns in development. Callers wrap `gridTemplate()` in their
 * own memo.
 */
export type SortKey =
  | 'favorite'
  | 'filename'
  | 'category'
  | 'ucsCategory'
  | 'ucsSubcategory'
  | 'durationS'
  | 'bpm'
  | 'channels'
  | 'sampleRate'
  | 'filesize'
  | 'description'

export interface Column {
  key: SortKey
  /** Resolved at render time: the header follows the locale. */
  label: () => string
  /** Width in CSS pixels. Fixed, not `fr`: see `gridTemplate`. */
  defaultWidth: number
  minWidth: number
  numeric?: boolean
  /**
   * Pinned columns are always first, never hidden and never resized. The star is
   * one glyph wide and carries no label; a user who dragged it to the far right
   * would have a column they cannot identify.
   */
  pinned?: boolean
  /** The star keeps its 32px; a resize handle on it would have nowhere to go. */
  resizable: boolean
}

export const COLUMNS: Column[] = [
  { key: 'favorite', label: () => '★', defaultWidth: 32, minWidth: 32, pinned: true, resizable: false },
  { key: 'filename', label: () => t('col.name'), defaultWidth: 236, minWidth: 120, resizable: true },
  { key: 'category', label: () => t('col.category'), defaultWidth: 100, minWidth: 64, resizable: true },
  { key: 'ucsCategory', label: () => t('col.ucsCategory'), defaultWidth: 92, minWidth: 56, resizable: true },
  { key: 'ucsSubcategory', label: () => t('col.ucsSubcategory'), defaultWidth: 104, minWidth: 64, resizable: true },
  { key: 'durationS', label: () => t('col.length'), defaultWidth: 74, minWidth: 56, numeric: true, resizable: true },
  { key: 'bpm', label: () => t('col.bpm'), defaultWidth: 52, minWidth: 44, numeric: true, resizable: true },
  { key: 'channels', label: () => t('col.channels'), defaultWidth: 46, minWidth: 44, resizable: true },
  { key: 'sampleRate', label: () => t('col.rate'), defaultWidth: 62, minWidth: 48, numeric: true, resizable: true },
  { key: 'filesize', label: () => t('col.size'), defaultWidth: 70, minWidth: 56, numeric: true, resizable: true },
  { key: 'description', label: () => t('col.description'), defaultWidth: 156, minWidth: 80, resizable: true }
]

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
  ucsSubcategory: 'ucs_subcategory',
  durationS: 'duration',
  bpm: 'bpm',
  channels: 'channels',
  sampleRate: 'sample_rate',
  filesize: 'filesize',
  description: 'description'
}

/**
 * Default direction per column, so the first click on a numeric column is the
 * useful one: nobody sorting by size wants the smallest file first.
 */
export const DEFAULT_DIR: Record<SortKey, 'asc' | 'desc'> = {
  favorite: 'desc',
  filename: 'asc',
  category: 'asc',
  ucsCategory: 'asc',
  ucsSubcategory: 'asc',
  durationS: 'desc',
  bpm: 'desc',
  channels: 'desc',
  sampleRate: 'desc',
  filesize: 'desc',
  description: 'asc'
}

const ORDER_KEY = 'mio:column-order'
const HIDDEN_KEY = 'mio:column-hidden'
const WIDTHS_KEY = 'mio:column-widths'
const SETTINGS_KEY = 'mio:settings'
const MAX_COL_W = 800

const ALL_KEYS: SortKey[] = COLUMNS.map((c) => c.key)
const COLUMN_BY_KEY = new Map(COLUMNS.map((c) => [c.key, c]))
const PINNED_KEYS = new Set(COLUMNS.filter((c) => c.pinned).map((c) => c.key))

/**
 * Where the columns sit, in the user's order.
 *
 * Stored as a plain list of keys rather than an index per column: a key that is
 * no longer in `COLUMNS` simply drops out on load, and a column added later
 * appears at the end instead of vanishing because it was not in a stale list.
 */
function loadColumnOrder(): SortKey[] {
  let stored: unknown
  try {
    const raw = localStorage.getItem(ORDER_KEY)
    if (!raw) return [...ALL_KEYS]
    stored = JSON.parse(raw)
  } catch {
    return [...ALL_KEYS]
  }
  if (!Array.isArray(stored)) return [...ALL_KEYS]
  const seen = [...new Set(stored.filter((k): k is SortKey => ALL_KEYS.includes(k as SortKey)))]
  const order = [...seen, ...ALL_KEYS.filter((k) => !seen.includes(k))]
  const favorite = order.indexOf('favorite')
  if (favorite > 0) order.unshift(...order.splice(favorite, 1))
  return order
}

/**
 * Hidden columns. The star and the filename are never hidden: a list with no name
 * column is a list of numbers.
 */
function loadHidden(): Set<SortKey> {
  const hidden = new Set<SortKey>()
  try {
    const raw = localStorage.getItem(HIDDEN_KEY)
    if (raw) {
      const parsed = JSON.parse(raw)
      if (Array.isArray(parsed)) {
        for (const key of parsed) {
          if (ALL_KEYS.includes(key as SortKey) && !PINNED_KEYS.has(key as SortKey)) {
            hidden.add(key as SortKey)
          }
        }
      }
      return hidden
    }
  } catch {
    return hidden
  }
  // The old build had one switch for the category column. A profile that had it
  // off should open with the column hidden, and the stale key is written out the
  // next time the settings dialog saves.
  try {
    const raw = localStorage.getItem(SETTINGS_KEY)
    const parsed = raw ? (JSON.parse(raw) as { showCategory?: unknown }) : null
    if (parsed && parsed.showCategory === false) hidden.add('category')
  } catch {
    // A corrupt blob costs the migration, not the list.
  }
  return hidden
}

/** Widths, clamped to each column's own range. A missing or bogus value is the default. */
function loadWidths(): Record<SortKey, number> {
  const widths = {} as Record<SortKey, number>
  for (const column of COLUMNS) widths[column.key] = column.defaultWidth
  try {
    const raw = localStorage.getItem(WIDTHS_KEY)
    const parsed = raw ? (JSON.parse(raw) as Record<string, unknown>) : null
    if (parsed && typeof parsed === 'object') {
      for (const column of COLUMNS) {
        const value = parsed[column.key]
        if (typeof value === 'number' && Number.isFinite(value)) {
          widths[column.key] = clampWidth(column, value)
        }
      }
    }
  } catch {
    // A corrupt entry costs the resized widths, not the table.
  }
  return widths
}

function clampWidth(column: Column, px: number): number {
  return Math.min(MAX_COL_W, Math.max(column.minWidth, Math.round(px)))
}

function store(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // A locked profile costs the arrangement, not the session.
  }
}

const [columnOrder, setColumnOrder] = createSignal<SortKey[]>(loadColumnOrder())
const [hiddenColumns, setHiddenColumns] = createSignal<Set<SortKey>>(loadHidden())
const [columnWidths, setColumnWidths] = createSignal<Record<SortKey, number>>(loadWidths())

/**
 * Columns actually drawn, which the header and every row must agree on.
 *
 * The favourite star is pinned first; the rest follow the stored order. Hiding is
 * a set, so a hidden column keeps its place in the order and reappears where the
 * user left it.
 */
export function visibleColumns(): Column[] {
  const hidden = hiddenColumns()
  const columns = columnOrder()
    .filter((key) => !hidden.has(key))
    .map((key) => COLUMN_BY_KEY.get(key))
    .filter((column): column is Column => column !== undefined)
  const pinned = columns.filter((column) => column.pinned)
  return [...pinned, ...columns.filter((column) => !column.pinned)]
}

export function widthOf(key: SortKey): number {
  const column = COLUMN_BY_KEY.get(key)
  const width = columnWidths()[key]
  if (!column) return 0
  return typeof width === 'number' ? width : column.defaultWidth
}

/**
 * The grid template shared by the header, the skeleton rows and the real rows.
 *
 * All pixels, never `fr` or percentages. A `fr` track is recomputed against the
 * viewport, so a width the user dragged would become a different width at another
 * window size; fixed tracks also make the three grids provably equal, which a
 * layout check can assert. The price is a horizontal scrollbar at small windows,
 * and that is deliberate — the alternative is widths that silently change.
 */
export function gridTemplate(): string {
  return visibleColumns()
    .map((column) => `${widthOf(column.key)}px`)
    .join(' ')
}

/** Sum of every visible track, in pixels; drives the scroll spacer's min-width. */
export function totalWidth(): number {
  return visibleColumns().reduce((sum, column) => sum + widthOf(column.key), 0)
}

/**
 * Move `from` next to `to`.
 *
 * `after` comes from which half of the target header the pointer is over, so a
 * column can be dropped at the far right by aiming at the right half of the last
 * one. Inserting always-before would make that position unreachable.
 *
 * Works on the full order, not the visible subset: splicing the visible list
 * dropped hidden columns out of the stored order, and the next load appended them
 * at the end for no reason the user could see.
 */
export function moveColumn(from: SortKey, to: SortKey, after: boolean): void {
  if (from === to || PINNED_KEYS.has(from) || PINNED_KEYS.has(to)) return
  const base = columnOrder().filter((key) => key !== from && !PINNED_KEYS.has(key))
  const at = base.indexOf(to)
  if (at < 0) return
  base.splice(after ? at + 1 : at, 0, from)
  const next = [...columnOrder().filter((key) => PINNED_KEYS.has(key)), ...base]
  setColumnOrder(next)
  store(ORDER_KEY, next)
}

export function setColumnWidth(key: SortKey, px: number): void {
  const column = COLUMN_BY_KEY.get(key)
  if (!column || !column.resizable) return
  // The set has to be a new object: mutating the current one does not notify
  // Solid's readers, and the symptom is "the drag did nothing but localStorage
  // already changed".
  setColumnWidths({ ...columnWidths(), [key]: clampWidth(column, px) })
}

/** Persist widths. Called on pointer-up, not per pointer-move. */
export function saveColumnWidths(): void {
  store(WIDTHS_KEY, columnWidths())
}

export function toggleColumn(key: SortKey): void {
  if (PINNED_KEYS.has(key)) return
  const next = new Set(hiddenColumns())
  if (next.has(key)) next.delete(key)
  else next.add(key)
  setHiddenColumns(next)
  store(HIDDEN_KEY, [...next])
}

export { hiddenColumns }
