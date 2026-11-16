import { For, Show, createMemo, createSignal } from 'solid-js'
import type { Facet } from './ipc'

/**
 * Assign a category by hand to one or many sounds.
 *
 * The input is a combobox over the categories that already exist, because a
 * hand-typed category that differs by a capital letter creates a second facet
 * group that looks identical in the rail. Subcategory is optional and only shown
 * once a category is chosen.
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

  return (
    <div class="modal-backdrop" onClick={props.onClose}>
      <div class="modal" onClick={(e) => e.stopPropagation()} role="dialog">
        <h2 class="modal-title">
          Set category
          <Show when={props.count > 1}>
            <span class="dim"> · {props.count} sounds</span>
          </Show>
        </h2>

        <label class="field">
          <span>Category</span>
          <input
            list="category-options"
            value={category()}
            placeholder="e.g. Weapons"
            autofocus
            onInput={(e) => setCategory(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') save()
              if (e.key === 'Escape') props.onClose()
            }}
          />
          <datalist id="category-options">
            <For each={categories()}>{(name) => <option value={name} />}</For>
          </datalist>
        </label>

        <label class="field">
          <span>Subcategory (optional)</span>
          <input
            list="subcategory-options"
            value={subcategory()}
            placeholder="e.g. Firearms"
            onInput={(e) => setSubcategory(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') save()
              if (e.key === 'Escape') props.onClose()
            }}
          />
          <datalist id="subcategory-options">
            <For each={subcategories()}>{(name) => <option value={name} />}</For>
          </datalist>
        </label>

        <div class="modal-actions">
          <button class="ghost" onClick={props.onClear}>
            Clear category
          </button>
          <div class="spacer" />
          <button class="ghost" onClick={props.onClose}>
            Cancel
          </button>
          <button class="primary" onClick={save} disabled={!category().trim()}>
            Save
          </button>
        </div>
      </div>
    </div>
  )
}
