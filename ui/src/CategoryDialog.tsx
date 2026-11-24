import { For, Show, createMemo, createSignal } from 'solid-js'
import { categoryName, t } from './i18n'
import type { Facet, UcsCategory } from './ipc'

/**
 * Assign a category by hand to one or many sounds.
 *
 * The input is a combobox over the categories that already exist, because a
 * hand-typed category that differs by a capital letter creates a second facet
 * group that looks identical in the rail. Subcategory is optional and only shown
 * once a category is chosen.
 *
 * The frame is an `ix-modal`; iX owns the backdrop, the sizing and the Escape
 * handling, and the footer buttons are `ix-button`s so they carry the design
 * system's focus ring and pressed states.
 */
interface Props {
  count: number
  facets: Facet[]
  ucsCategories: UcsCategory[]
  onSave: (category: string, subcategory: string | null) => void
  onSaveUcs: (value: UcsCategory) => void
  onClear: () => void
  onClose: () => void
}

export function CategoryDialog(props: Props) {
  const [category, setCategory] = createSignal('')
  const [subcategory, setSubcategory] = createSignal('')
  const [ucsCatId, setUcsCatId] = createSignal('')
  let modal: (HTMLElement & { showModal: () => Promise<void>; closeModal: (r?: unknown) => Promise<void> }) | undefined

  const categories = createMemo(() => {
    const seen = new Set<string>()
    for (const facet of props.facets) seen.add(facet.category)
    return [...seen].sort((a, b) => a.localeCompare(b))
  })

  const subcategories = createMemo(() => {
    const chosen = category().trim().toLowerCase()
    if (!chosen) return []
    const seen = new Set<string>()
    for (const facet of props.facets) {
      if (facet.category.toLowerCase() === chosen && facet.subcategory) {
        seen.add(facet.subcategory)
      }
    }
    return [...seen].sort((a, b) => a.localeCompare(b))
  })

  const save = () => {
    const value = category().trim()
    if (!value) return
    const sub = subcategory().trim()
    props.onSave(value, sub === '' ? null : sub)
  }

  const selectedUcs = () => props.ucsCategories.find((entry) => entry.catId === ucsCatId())

  // iX modals are opened imperatively: the element is rendered, then `showModal`
  // moves it into the top layer and animates it in. A `queueMicrotask` is enough
  // because Solid has already committed the element by the time this effect runs.
  const open = (el: HTMLElement) => {
    modal = el as typeof modal
    queueMicrotask(() => void modal?.showModal())
  }

  const close = (then?: () => void) => {
    void modal?.closeModal()
    then?.()
  }

  /**
   * iX dismisses itself on the header's × and on Escape. Without this the parent
   * keeps `categoryDialog` set, and the next right-click on a row would render a
   * dialog whose element is already dismissed.
   */
  const onDialogClosed = () => props.onClose()

  return (
    <ix-modal
      ref={open}
      size="480"
      on:dialogClose={onDialogClosed}
      on:dialogDismiss={onDialogClosed}
    >
      <ix-modal-header heading={t('category.heading')} />
      <ix-modal-content>
        <div class="modal-form">
          <div class="setting-hint">{t('category.selected', { count: props.count })}</div>
          <label class="field">
            <span class="field-label">{t('category.label')}</span>
            <ix-input
              id="category-input"
              value={category()}
              placeholder={t('category.placeholder')}
              helperText={
                categories().length > 0
                  ? t('category.existing', { names: categories().slice(0, 4).join(', ') })
                  : undefined
              }
              on:valueChange={(e: CustomEvent<string>) => setCategory(e.detail)}
              onKeyDown={(e: KeyboardEvent) => {
                if (e.key === 'Enter') save()
              }}
            />
          </label>

          <label class="field">
            <span class="field-label">{t('category.ucsLabel')}</span>
            <ix-select
              id="ucs-category-input"
              value={ucsCatId()}
              on:valueChange={(e: CustomEvent<string>) => setUcsCatId(e.detail)}
            >
              <ix-select-item value="" label={t('category.noUcsLabel')} />
              <For each={props.ucsCategories}>
                {(entry) => (
                  <ix-select-item
                    value={entry.catId}
                    label={`${entry.catId} · ${entry.category}${entry.subcategory ? ` / ${entry.subcategory}` : ''}`}
                  />
                )}
              </For>
            </ix-select>
          </label>
          <Show when={selectedUcs()}>
            {(entry) => (
              <div class="setting-hint">
                {entry().projectedCategory}
                <Show when={entry().projectedSubcategory}> / {entry().projectedSubcategory}</Show>
              </div>
            )}
          </Show>

          <label class="field">
            <span class="field-label">{t('category.subLabel')}</span>
            <ix-input
              id="subcategory-input"
              value={subcategory()}
              placeholder={t('category.subPlaceholder')}
              helperText={
                subcategories().length > 0
                  ? t('category.existing', { names: subcategories().slice(0, 4).join(', ') })
                  : undefined
              }
              on:valueChange={(e: CustomEvent<string>) => setSubcategory(e.detail)}
              onKeyDown={(e: KeyboardEvent) => {
                if (e.key === 'Enter') save()
              }}
            />
          </label>

          {/* The whole vocabulary, so a typo cannot silently create a duplicate
              facet. Two chips rows read faster than a datalist dropdown. */}
          <Show when={categories().length > 0}>
            <div class="field">
              <span class="field-label">{t('category.pickExisting')}</span>
              <div style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '4px' }}>
                <For each={categories()}>
                  {(name) => (
                    <ix-chip
                      variant="neutral"
                      outline
                      onClick={() => {
                        setCategory(name)
                        setSubcategory('')
                      }}
                    >
                      {categoryName(name)}
                    </ix-chip>
                  )}
                </For>
              </div>
            </div>
          </Show>

          <Show when={subcategories().length > 0}>
            <div class="field">
              <span class="field-label">{t('category.subcategoriesOf', { name: category() })}</span>
              <div style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '4px' }}>
                <For each={subcategories()}>
                  {(name) => (
                    <ix-chip variant="neutral" outline onClick={() => setSubcategory(name)}>
                      {categoryName(name)}
                    </ix-chip>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>
      </ix-modal-content>
      <ix-modal-footer>
        <div class="modal-actions">
          <ix-button variant="danger-secondary" onClick={() => close(props.onClear)}>
            {t('category.clear')}
          </ix-button>
          <div class="spacer" />
          <ix-button variant="secondary" onClick={() => close(props.onClose)}>
            {t('common.cancel')}
          </ix-button>
          <ix-button
            variant="primary"
            disabled={!category().trim() && !selectedUcs()}
            onClick={() => close(selectedUcs() ? () => props.onSaveUcs(selectedUcs()!) : save)}
          >
            {t('common.save')}
          </ix-button>
        </div>
      </ix-modal-footer>
    </ix-modal>
  )
}
