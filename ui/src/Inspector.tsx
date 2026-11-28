import { For, Show, createEffect, createMemo, createSignal, onCleanup } from 'solid-js'
import { categoryName, channelLabel, t } from './i18n'
import { api, type SoundDetails, type SoundRow, type Tag } from './ipc'
import { formatBpm, formatBytes, sampleRateLabel } from './format'
import { ICON } from './icons'

/**
 * The details inspector: a right-hand column describing the active row.
 *
 * Every click semantics lives in `App`. This component renders values and calls
 * the matching callback; it never writes a filter itself. The reason is the same
 * one that moved column state into `columns.ts`: an inspector that edited
 * `queryTerms` directly would be a second owner of the search, and the two copies
 * would disagree the first time the rail cleared a filter.
 *
 * Its own state is only the lazily fetched detail payload (tags, cover), which no
 * other component needs and which must be discarded the moment the row changes —
 * showing the previous file's cover for one frame is a wrong picture, not a slow
 * one.
 */
export interface InspectorProps {
  /** The active row; the inspector shows it even when it is not the selection. */
  row: SoundRow | null
  /** How many rows are selected. `1` or `0` hides the count. */
  selectionCount: number
  open: boolean
  /** Saved panel width in CSS pixels; the layout clamps it to the window. */
  width: number
  /** Bumped when the details pass finishes or the library changes. */
  coverEpoch: number
  onOpen: () => void
  onClose: () => void
  onResize: (px: number) => void
  /** Append a value to the search terms. */
  onQueryTerm: (text: string) => void
  /** Copy a value and show the existing notice. */
  onCopy: (value: string) => void
  /** Replace the filter with this absolute folder. */
  onFolder: (absolute: string) => void
  /** Replace the filter with this cover hash. */
  onCover: (key: string) => void
  onCategory: (category: string) => void
  onSubcategory: (category: string, sub: string) => void
  onUcsCategory: (category: string) => void
  onUcsSubcategory: (category: string, sub: string) => void
  onUcsCatId: (catId: string) => void
  /** Numeric filters. The panel shows the value as plain text when absent. */
  onChannels?: (channels: number) => void
  onSampleRate?: (rate: number) => void
  onBpm?: (bpm: number) => void
}

/** A clickable value: a button that looks like text until hovered. */
function Value(props: {
  value: string
  title?: string
  onClick?: () => void
  /** A copy row reads differently from a filtering row in the tooltip only. */
  dim?: boolean
}) {
  if (!props.onClick) {
    return (
      <span class="inspector-value static" title={props.title}>
        {props.value}
      </span>
    )
  }
  return (
    <button
      class="inspector-value"
      classList={{ dim: props.dim }}
      title={props.title}
      onClick={() => props.onClick?.()}
    >
      {props.value}
    </button>
  )
}

function Row(props: { label: string; children: unknown }) {
  return (
    <div class="inspector-row">
      <span class="inspector-label">{props.label}</span>
      <div class="inspector-field">{props.children as never}</div>
    </div>
  )
}

/**
 * Known tag keys, by group. Unknown keys keep their raw code and get their own
 * section named after the source: inventing a friendly name for `TXXX:eng` would
 * claim a meaning the file never gave.
 */
const KNOWN_TAGS: Record<string, string> = {
  artist: 'tag.artist',
  album: 'tag.album',
  album_artist: 'tag.albumArtist',
  title: 'tag.title',
  track: 'tag.track',
  track_total: 'tag.trackTotal',
  disc: 'tag.disc',
  genre: 'tag.genre',
  comment: 'tag.comment',
  description: 'tag.description',
  composer: 'tag.composer',
  producer: 'tag.producer',
  writer: 'tag.writer',
  label: 'tag.label',
  date: 'tag.date',
  copyright: 'tag.copyright',
  encoder: 'tag.encoder',
  encoded_by: 'tag.encodedBy',
  isrc: 'tag.isrc',
  catalog_number: 'tag.catalogNumber',
  bpm: 'tag.bpm',
  mood: 'tag.mood',
  language: 'tag.language',
  license: 'tag.license',
  original_artist: 'tag.originalArtist',
  rating: 'tag.rating',
  version: 'tag.version',
  lyrics: 'tag.lyrics',
  'bext.description': 'tag.bextDescription',
  'bext.originator': 'tag.bextOriginator',
  'bext.originator_reference': 'tag.bextOriginatorReference',
  'bext.origination_date': 'tag.bextDate',
  'bext.origination_time': 'tag.bextTime',
  'bext.time_reference': 'tag.bextTimeReference',
  'bext.coding_history': 'tag.bextHistory',
  'ixml.project': 'tag.project',
  'ixml.scene': 'tag.scene',
  'ixml.take': 'tag.take',
  'ixml.tape': 'tag.tape',
  'ixml.note': 'tag.note',
  'ixml.circled': 'tag.circled',
  'ixml.category': 'tag.ixmlCategory',
  'ixml.subcategory': 'tag.ixmlSubcategory',
  'ixml.speed': 'tag.speed',
  'ixml.track': 'tag.trackName',
  'ixml.roll': 'tag.roll',
  'info.artist': 'tag.artist',
  'info.comment': 'tag.comment',
  'info.copyright': 'tag.copyright',
  'info.date': 'tag.date',
  'info.genre': 'tag.genre',
  'info.title': 'tag.title',
  'info.album': 'tag.album',
  'info.software': 'tag.software',
  'info.source': 'tag.source',
  'info.technician': 'tag.technician',
  'info.commissioner': 'tag.commissioner',
  'info.engineer': 'tag.engineer',
  'info.subject': 'tag.subject',
  'info.medium': 'tag.medium'
}

/** The source headings, in the order the panel shows them. */
const SOURCE_ORDER = ['id3', 'vorbis', 'm4a', 'bext', 'ixml', 'info']

function tagLabel(tag: Tag): string {
  const key = KNOWN_TAGS[tag.key]
  if (key) return t(key as Parameters<typeof t>[0])
  // An unknown code is still a key: show it verbatim, so the panel says what the
  // file says rather than a guess.
  return tag.key
}

export function Inspector(props: InspectorProps) {
  const [details, setDetails] = createSignal<SoundDetails | null>(null)
  const [loading, setLoading] = createSignal(false)

  /**
   * The row's directory, in native separators.
   *
   * `SoundRow.folder` is relative to the library root (store format), so it is
   * readable but not directly filterable: the backend's folder filter compares
   * absolute path ranges. The directory taken from the path is the absolute value
   * the click needs, and it cannot drift from the path the way a re-joined
   * root would when the root was spelled with the other separator.
   */
  const directory = createMemo(() => {
    const path = props.row?.path
    if (!path) return null
    const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
    return cut > 0 ? path.slice(0, cut) : null
  })

  const relativeFolder = createMemo(() => {
    const folder = props.row?.folder
    return folder && folder.length > 0 ? folder : null
  })

  const stem = createMemo(() => {
    const name = props.row?.filename ?? ''
    return name.replace(/\.[^.\\/]+$/, '')
  })

  let panel: HTMLElement | undefined

  /**
   * Drag the panel's left edge.
   *
   * Listeners live on `window`, like the rail's: the pointer leaves the 4px edge
   * within a frame, and a lost pointer would leave the panel stuck at whatever
   * width the cursor happened to pass.
   */
  const startResize = (event: PointerEvent) => {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = props.width
    const move = (e: PointerEvent) => {
      props.onResize(startWidth + (startX - e.clientX))
    }
    const finish = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', finish)
      window.removeEventListener('pointercancel', finish)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', finish)
    window.addEventListener('pointercancel', finish)
  }

  /**
   * The detail payload for the active row.
   *
   * Closed panel: no request at all. On a row change the previous payload is
   * dropped before the timer, so a stale cover or a stale tag list cannot be
   * painted under the new name. The debounce is short — an arrow-key walk should
   * feel live — and a generation counter discards a response that lost the race.
   */
  let generation = 0
  createEffect(() => {
    const path = props.row?.path ?? null
    const epoch = props.coverEpoch
    const open = props.open
    generation += 1
    const mine = generation
    setDetails(null)
    setLoading(false)
    if (panel) panel.scrollTop = 0
    if (!open || !path) return
    setLoading(true)
    const timer = window.setTimeout(() => {
      if (mine !== generation) return
      void api
        .inspectSound(path)
        .then((value) => {
          if (mine !== generation) return
          setDetails(value)
        })
        .catch(() => {
          if (mine !== generation) return
          setDetails(null)
        })
        .finally(() => {
          if (mine === generation) setLoading(false)
        })
    }, 90)
    void epoch
    onCleanup(() => {
      window.clearTimeout(timer)
      generation += 1
    })
  })

  /** The cover key the row's filter would use: the payload's, else the row's. */
  const coverKey = createMemo(() => details()?.cover?.key ?? props.row?.coverKey ?? null)

  const coverUrl = createMemo(() => details()?.cover?.dataUrl ?? null)

  /**
   * Tags grouped by source.
   *
   * Sources keep a fixed order rather than first-seen order: a WAV read as
   * `bext` + `ixml` + `info` should list them in the same sequence every time,
   * and a file with only `id3` should not look like a different kind of file.
   */
  const tagGroups = createMemo(() => {
    const payload = details()
    if (!payload) return []
    const groups = new Map<string, Tag[]>()
    for (const tag of payload.tags) {
      const list = groups.get(tag.source) ?? []
      list.push(tag)
      groups.set(tag.source, list)
    }
    const ordered = [...groups.entries()].sort((a, b) => {
      const ia = SOURCE_ORDER.indexOf(a[0])
      const ib = SOURCE_ORDER.indexOf(b[0])
      return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib)
    })
    return ordered
  })

  return (
    <Show
      when={props.open}
      fallback={
        <button
          class="inspector-strip"
          aria-expanded="false"
          aria-controls="inspector"
          title={t('inspector.open')}
          onClick={() => props.onOpen()}
        >
          {/* The strip is vertical, so a chevron above and below the title reads as
              "this panel pulls out to the left". One at the far end only would look
              like a scroll arrow. */}
          <ix-icon name={ICON.chevronLeft} size="12" />
          <span>{t('inspector.title')}</span>
          <ix-icon name={ICON.chevronLeft} size="12" />
        </button>
      }
    >
      <aside
        class="inspector"
        id="inspector"
        aria-label={t('inspector.title')}
        ref={(el) => (panel = el)}
        style={{ width: `${props.width}px` }}
      >
        <div
          class="inspector-resize"
          title={t('inspector.resize')}
          onPointerDown={startResize}
        />
        <div class="inspector-head">
          <span class="inspector-title">{t('inspector.title')}</span>
          <button
            class="inspector-close"
            aria-expanded="true"
            aria-controls="inspector"
            title={t('inspector.close')}
            onClick={() => props.onClose()}
          >
            <ix-icon name={ICON.chevronRight} size="16" />
          </button>
        </div>

        <Show
          when={props.row}
          fallback={<div class="inspector-empty">{t('inspector.empty')}</div>}
        >
          {(row) => (
            <div class="inspector-body">
              <Show when={props.selectionCount > 1}>
                <div class="inspector-count">
                  {t('status.selected', { count: props.selectionCount.toLocaleString() })}
                </div>
              </Show>

              {/* A skeleton while the 512px picture decodes; no block at all once
                  the payload says there is no cover, so a coverless row does not
                  reserve a square of empty space. */}
              <Show when={loading() || coverUrl()}>
                <button
                  class="inspector-cover"
                  classList={{ placeholder: !coverUrl() }}
                  title={coverKey() ? t('inspector.sameCover') : undefined}
                  onClick={() => coverKey() && props.onCover(coverKey()!)}
                >
                  <Show when={coverUrl()}>
                    {(url) => <img src={url()} alt="" />}
                  </Show>
                </button>
              </Show>

              <Row label={t('inspector.name')}>
                <Value
                  value={row().filename}
                  title={t('inspector.searchName')}
                  onClick={() => props.onQueryTerm(stem())}
                />
              </Row>
              <Row label={t('inspector.path')}>
                <Value
                  value={row().path}
                  title={t('inspector.copyPath')}
                  dim
                  onClick={() => props.onCopy(row().path)}
                />
              </Row>
              <Show when={relativeFolder()}>
                {(folder) => (
                  <Row label={t('inspector.folder')}>
                    <Value
                      value={folder()}
                      title={t('inspector.filterFolder')}
                      onClick={() => directory() && props.onFolder(directory()!)}
                    />
                  </Row>
                )}
              </Show>

              <Row label={t('inspector.duration')}>
                <Value
                  value={row().durationLabel}
                  title={t('inspector.copyValue')}
                  dim
                  onClick={() => props.onCopy(row().durationLabel)}
                />
              </Row>
              <Row label={t('inspector.sampleRate')}>
                <Value
                  value={sampleRateLabel(row().sampleRate)}
                  title={props.onSampleRate ? t('inspector.filterSampleRate') : undefined}
                  onClick={
                    props.onSampleRate && row().sampleRate !== null
                      ? () => props.onSampleRate!(row().sampleRate!)
                      : undefined
                  }
                />
              </Row>
              <Row label={t('inspector.channels')}>
                <Value
                  value={channelLabel(row().channels)}
                  title={props.onChannels ? t('inspector.filterChannels') : undefined}
                  onClick={
                    props.onChannels && row().channels !== null
                      ? () => props.onChannels!(row().channels!)
                      : undefined
                  }
                />
              </Row>
              <Row label={t('inspector.bpm')}>
                <Value
                  value={formatBpm(row().bpm)}
                  title={props.onBpm ? t('inspector.filterBpm') : undefined}
                  onClick={
                    props.onBpm && row().bpm !== null ? () => props.onBpm!(row().bpm!) : undefined
                  }
                />
              </Row>
              <Row label={t('inspector.filesize')}>
                <Value
                  value={formatBytes(row().filesize)}
                  title={t('inspector.copyValue')}
                  dim
                  onClick={() => props.onCopy(formatBytes(row().filesize))}
                />
              </Row>

              <Row label={t('inspector.category')}>
                <Value
                  value={categoryName(row().category)}
                  title={t('inspector.filterCategory')}
                  onClick={() => props.onCategory(row().category)}
                />
              </Row>
              <Show when={row().subcategory}>
                {(sub) => (
                  <Row label={t('inspector.subcategory')}>
                    <Value
                      value={sub()}
                      title={t('inspector.filterSubcategory')}
                      onClick={() => props.onSubcategory(row().category, sub())}
                    />
                  </Row>
                )}
              </Show>
              <Row label={t('inspector.confidence')}>
                <span class="inspector-value static">
                  {(row().confidence * 100).toFixed(0)}%
                </span>
              </Row>
              <Show when={row().ucsCategory}>
                {(ucs) => (
                  <Row label={t('inspector.ucsCategory')}>
                    <Value
                      value={ucs()}
                      title={t('inspector.filterUcsCategory')}
                      onClick={() => props.onUcsCategory(ucs())}
                    />
                  </Row>
                )}
              </Show>
              <Show when={row().ucsSubcategory}>
                {(sub) => (
                  <Row label={t('inspector.ucsSubcategory')}>
                    <Value
                      value={sub()}
                      title={t('inspector.filterUcsSubcategory')}
                      onClick={() => props.onUcsSubcategory(row().ucsCategory ?? '', sub())}
                    />
                  </Row>
                )}
              </Show>
              <Show when={row().ucsCatId}>
                {(catId) => (
                  <Row label={t('inspector.ucsCatId')}>
                    <Value
                      value={catId()}
                      title={t('inspector.filterUcsCatId')}
                      onClick={() => props.onUcsCatId(catId())}
                    />
                  </Row>
                )}
              </Show>
              <Show when={row().categoryManual}>
                <Row label={t('inspector.manual')}>
                  <span class="inspector-value static">{t('inspector.manualYes')}</span>
                </Row>
              </Show>

              <Show when={row().description}>
                {(description) => (
                  <div class="inspector-block">
                    <span class="inspector-label">{t('inspector.description')}</span>
                    <button
                      class="inspector-description"
                      title={description()}
                      onClick={() => props.onQueryTerm(description())}
                    >
                      {description()}
                    </button>
                  </div>
                )}
              </Show>

              <Show when={row().matchedTerms.length > 0}>
                <div class="inspector-block">
                  <span class="inspector-label">{t('inspector.matchedTerms')}</span>
                  <div class="inspector-terms">
                    <For each={row().matchedTerms}>
                      {(term) => (
                        <button
                          class="inspector-term"
                          title={t('inspector.searchTerm', { term })}
                          onClick={() => props.onQueryTerm(term)}
                        >
                          {term}
                        </button>
                      )}
                    </For>
                  </div>
                </div>
              </Show>

              <Show when={tagGroups().length > 0}>
                <div class="inspector-block">
                  <span class="inspector-label">{t('inspector.tags')}</span>
                  <Show when={details()?.lazy}>
                    <span class="inspector-hint">{t('inspector.tagsLazy')}</span>
                  </Show>
                  <For each={tagGroups()}>
                    {([source, tags]) => (
                      <div class="inspector-tag-group">
                        <span class="inspector-tag-source">
                          {t(`tagSource.${source}` as Parameters<typeof t>[0])}
                        </span>
                        <For each={tags}>
                          {(tag) => (
                            <div class="inspector-tag">
                              <span class="inspector-tag-key" title={tag.key}>
                                {tagLabel(tag)}
                              </span>
                              <Show
                                when={tag.searchable}
                                fallback={
                                  <span class="inspector-tag-value" title={tag.value}>
                                    {tag.value}
                                  </span>
                                }
                              >
                                <button
                                  class="inspector-tag-value searchable"
                                  title={t('inspector.searchTerm', { term: tag.value })}
                                  onClick={() => props.onQueryTerm(tag.value)}
                                >
                                  {tag.value}
                                </button>
                              </Show>
                            </div>
                          )}
                        </For>
                      </div>
                    )}
                  </For>
                  <Show when={details()?.tagsTruncated}>
                    <span class="inspector-hint">{t('inspector.tagsTruncated')}</span>
                  </Show>
                </div>
              </Show>
            </div>
          )}
        </Show>
      </aside>
    </Show>
  )
}
