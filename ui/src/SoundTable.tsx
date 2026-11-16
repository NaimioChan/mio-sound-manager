import { For, Show, createSignal, createMemo } from 'solid-js'
import type { SoundRow } from './ipc'

/**
 * The sound list.
 *
 * Virtualised by row window rather than by a table library: the index can return
 * thousands of rows and the reference implementation built its own windowing too.
 * Fixed row height keeps the maths exact, which matters because a mis-measured
 * row shows up as a jump when scrolling.
 */
const ROW_H = 26
const OVERSCAN = 8

interface Props {
  rows: SoundRow[]
  selected: SoundRow | null
  onSelect: (row: SoundRow) => void
}

type SortKey = 'favorite' | 'filename' | 'category' | 'durationS' | 'channels' | 'sampleRate' | 'filesize'

const COLUMNS: { key: SortKey; label: string; width: string; numeric?: boolean }[] = [
  { key: 'favorite', label: '★', width: '32px' },
  { key: 'filename', label: 'Name', width: 'minmax(180px, 2fr)' },
  { key: 'category', label: 'Category', width: 'minmax(140px, 1fr)' },
  { key: 'durationS', label: 'Length', width: '82px', numeric: true },
  { key: 'channels', label: 'Ch', width: '62px' },
  { key: 'sampleRate', label: 'Rate', width: '76px', numeric: true },
  { key: 'filesize', label: 'Size', width: '82px', numeric: true }
]

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

export function SoundTable(props: Props) {
  const [scrollTop, setScrollTop] = createSignal(0)
  const [viewportH, setViewportH] = createSignal(600)
  const [sortKey, setSortKey] = createSignal<SortKey>('filename')
  const [sortAsc, setSortAsc] = createSignal(true)

  const sorted = createMemo(() => {
    const key = sortKey()
    const dir = sortAsc() ? 1 : -1
    const copy = props.rows.slice()
    copy.sort((a, b) => {
      const av = a[key]
      const bv = b[key]
      if (av === bv) return a.filename.localeCompare(b.filename) * dir
      if (av === null || av === undefined) return 1
      if (bv === null || bv === undefined) return -1
      if (typeof av === 'number' && typeof bv === 'number') return (av - bv) * dir
      return String(av).localeCompare(String(bv)) * dir
    })
    return copy
  })

  const range = createMemo(() => {
    const total = sorted().length
    const first = Math.max(0, Math.floor(scrollTop() / ROW_H) - OVERSCAN)
    const visible = Math.ceil(viewportH() / ROW_H) + OVERSCAN * 2
    const last = Math.min(total, first + visible)
    return { first, last }
  })

  const toggleSort = (key: SortKey) => {
    if (sortKey() === key) {
      setSortAsc(!sortAsc())
    } else {
      setSortKey(key)
      setSortAsc(true)
    }
  }

  return (
    <div class="table">
      <div
        class="table-head"
        style={{ 'grid-template-columns': COLUMNS.map((c) => c.width).join(' ') }}
      >
        <For each={COLUMNS}>
          {(col) => (
            <button
              class={col.numeric ? 'head-cell numeric' : 'head-cell'}
              onClick={() => toggleSort(col.key)}
            >
              {col.label}
              <Show when={sortKey() === col.key}>
                <span class="sort-arrow">{sortAsc() ? '▲' : '▼'}</span>
              </Show>
            </button>
          )}
        </For>
      </div>

      <div
        class="table-body"
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        ref={(el) => {
          const observer = new ResizeObserver(() => setViewportH(el.clientHeight))
          observer.observe(el)
          setViewportH(el.clientHeight)
        }}
      >
        <div style={{ height: `${sorted().length * ROW_H}px`, position: 'relative' }}>
          <For each={sorted().slice(range().first, range().last)}>
            {(row, index) => {
              const isSelected = () => props.selected?.path === row.path
              return (
                <div
                  class={isSelected() ? 'row selected' : 'row'}
                  style={{
                    position: 'absolute',
                    top: `${(range().first + index()) * ROW_H}px`,
                    height: `${ROW_H}px`,
                    'grid-template-columns': COLUMNS.map((c) => c.width).join(' ')
                  }}
                  onClick={() => props.onSelect(row)}
                >
                  <span class="cell favorite">{row.favorite ? '★' : ''}</span>
                  <span class="cell name" title={row.path}>
                    {row.filename}
                  </span>
                  <span class="cell dim" title={row.matchedTerms.join(', ')}>
                    {row.category}
                    <Show when={row.subcategory}>
                      <span class="sub"> / {row.subcategory}</span>
                    </Show>
                  </span>
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
    </div>
  )
}
