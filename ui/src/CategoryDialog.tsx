import { For, Show, createMemo, createSignal } from 'solid-js'
import type { Facet } from './ipc'

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
  onSave: (category: string, subcategory: string | null) => void
  onClear: () => void
  onClose: () => void
}

export function CategoryDialog(props: Props) {
  const [category, setCategory] = createSignal('')
  const [subcategory, setSubcategory] = createSignal('')
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

  return (
    <ix-modal ref={open} size="480">
      <ix-modal-header
        heading="Set category"
        subHeading={props.count > 1 ? `${props.count} sounds` : '1 sound'}
      />
      <ix-modal-content>
        <div class="modal-form">
          <label class="field">
            <span class="field-label">Category</span>
            <ix-input
              id="category-input"
              value={category()}
              placeholder="e.g. Weapons"
              helperText={categories().length > 0 ? `Existing: ${categories().slice(0, 4).join(', ')}` : undefined}
              on:valueChange={(e: CustomEvent<string>) => setCategory(e.detail)}
              onKeyDown={(e: KeyboardEvent) => {
                if (e.key === 'Enter') save()
              }}
            />
          </label>

          <label class="field">
            <span class="field-label">Subcategory (optional)</span>
            <ix-input
              id="subcategory-input"
              value={subcategory()}
              placeholder="e.g. Firearms"
              helperText={
                subcategories().length > 0
                  ? `Existing: ${subcategories().slice(0, 4).join(', ')}`
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
              <span class="field-label">Pick an existing one</span>
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
                      {name}
                    </ix-chip>
                  )}
                </For>
              </div>
            </div>
          </Show>

          <Show when={subcategories().length > 0}>
            <div class="field">
              <span class="field-label">Subcategories of {category()}</span>
              <div style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '4px' }}>
                <For each={subcategories()}>
                  {(name) => (
                    <ix-chip variant="neutral" outline onClick={() => setSubcategory(name)}>
                      {name}
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
            Clear category
          </ix-button>
          <div class="spacer" />
          <ix-button variant="secondary" onClick={() => close(props.onClose)}>
            Cancel
          </ix-button>
          <ix-button variant="primary" disabled={!category().trim()} onClick={() => close(save)}>
            Save
          </ix-button>
        </div>
      </ix-modal-footer>
    </ix-modal>
  )
}
