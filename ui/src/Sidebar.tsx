import { For, Show, createMemo, createSignal } from 'solid-js'
import type { Facet, FolderNode, LibraryStatus } from './ipc'

/**
 * The left rail: categories or folders, plus the library facts.
 *
 * Two modes rather than two panels, matching the reference implementation: a
 * sample library is browsed by *kind of sound* most of the time and by *where it
 * came from* when hunting a specific pack, and showing both at once halves the
 * height available for either. The mode is remembered across launches.
 */
type Mode = 'categories' | 'folders'

const MODE_KEY = 'reslip:sidebar-mode'

interface Props {
  folders: FolderNode | null
  library: LibraryStatus | null
  facets: Facet[]
  favorites: number
  total: number
  category: string | null
  subcategory: string | null
  folder: string | null
  favoritesOnly: boolean
  onCategory: (category: string | null, subcategory: string | null) => void
  onFolder: (folder: string | null) => void
  onToggleFavorites: () => void
}

/** One category with its subcategories, aggregated from the flat facet list. */
interface CategoryGroup {
  name: string
  count: number
  children: { name: string; count: number }[]
}

export function Sidebar(props: Props) {
  const [mode, setMode] = createSignal<Mode>(
    localStorage.getItem(MODE_KEY) === 'folders' ? 'folders' : 'categories'
  )
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set())

  const changeMode = (next: Mode) => {
    setMode(next)
    localStorage.setItem(MODE_KEY, next)
  }

  const toggle = (path: string) => {
    const next = new Set(expanded())
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setExpanded(next)
  }

  /**
   * The index returns one row per `(category, subcategory)`. The rail wants
   * category totals with subcategories nested, so aggregate here rather than
   * asking for a second query shape. `Uncategorized` sorts last: it is the
   * leftover bucket, not a kind of sound.
   */
  const groups = createMemo<CategoryGroup[]>(() => {
    const byName = new Map<string, CategoryGroup>()
    for (const facet of props.facets) {
      let group = byName.get(facet.category)
      if (!group) {
        group = { name: facet.category, count: 0, children: [] }
        byName.set(facet.category, group)
      }
      group.count += facet.count
      if (facet.subcategory) {
        group.children.push({ name: facet.subcategory, count: facet.count })
      }
    }
    const list = [...byName.values()]
    for (const group of list) group.children.sort((a, b) => b.count - a.count)
    list.sort((a, b) => {
      if (a.name === 'Uncategorized') return 1
      if (b.name === 'Uncategorized') return -1
      return b.count - a.count
    })
    return list
  })

  const clickCategory = (name: string) => {
    if (props.category === name && props.subcategory === null) {
      props.onCategory(null, null)
    } else {
      props.onCategory(name, null)
    }
  }

  const clickSubcategory = (category: string, subcategory: string) => {
    if (props.category === category && props.subcategory === subcategory) {
      props.onCategory(category, null)
    } else {
      props.onCategory(category, subcategory)
    }
  }

  const renderNode = (node: FolderNode, depth: number) => (
    <div class="folder-node">
      <button
        class={props.folder === node.path ? 'folder selected' : 'folder'}
        style={{ 'padding-left': `${8 + depth * 12}px` }}
        onClick={() => props.onFolder(node.path)}
        title={node.path}
      >
        <span
          class="twisty"
          onClick={(e) => {
            e.stopPropagation()
            toggle(node.path)
          }}
        >
          <Show when={node.children.length > 0} fallback={<span class="twisty-spacer" />}>
            {expanded().has(node.path) ? '▾' : '▸'}
          </Show>
        </span>
        <span class="folder-name">{node.name}</span>
        <span class="count">{node.count.toLocaleString()}</span>
      </button>

      <Show when={expanded().has(node.path)}>
        <For each={node.children}>{(child) => renderNode(child, depth + 1)}</For>
      </Show>
    </div>
  )

  return (
    <aside class="sidebar">
      <div class="side-modes">
        <button
          class={mode() === 'categories' ? 'side-mode on' : 'side-mode'}
          onClick={() => changeMode('categories')}
        >
          Categories
        </button>
        <button
          class={mode() === 'folders' ? 'side-mode on' : 'side-mode'}
          onClick={() => changeMode('folders')}
        >
          Folders
        </button>
      </div>

      <Show when={props.library} fallback={<div class="side-empty">no library open</div>}>
        <Show when={mode() === 'categories'}>
          <section class="side-section grow">
            <button
              class={props.favoritesOnly ? 'facet selected' : 'facet'}
              onClick={props.onToggleFavorites}
              title="Show favorites only"
            >
              <span class="facet-name">★ Favorites</span>
              <span class="count">{props.favorites.toLocaleString()}</span>
            </button>
            <button
              class={
                props.category === null && !props.favoritesOnly ? 'facet selected' : 'facet'
              }
              onClick={() => props.onCategory(null, null)}
            >
              <span class="facet-name">All sounds</span>
              <span class="count">{props.total.toLocaleString()}</span>
            </button>

            <For each={groups()}>
              {(group) => (
                <>
                  <button
                    class={
                      props.category === group.name && props.subcategory === null
                        ? 'facet selected'
                        : 'facet'
                    }
                    onClick={() => clickCategory(group.name)}
                    title={`${group.name} (${group.count.toLocaleString()})`}
                  >
                    <span
                      class="twisty"
                      onClick={(e) => {
                        e.stopPropagation()
                        toggle(group.name)
                      }}
                    >
                      <Show
                        when={group.children.length > 0}
                        fallback={<span class="twisty-spacer" />}
                      >
                        {expanded().has(group.name) ? '▾' : '▸'}
                      </Show>
                    </span>
                    <span class="facet-name">{group.name}</span>
                    <span class="count">{group.count.toLocaleString()}</span>
                  </button>

                  <Show when={expanded().has(group.name)}>
                    <For each={group.children}>
                      {(child) => (
                        <button
                          class={
                            props.category === group.name && props.subcategory === child.name
                              ? 'facet sub selected'
                              : 'facet sub'
                          }
                          onClick={() => clickSubcategory(group.name, child.name)}
                          title={`${group.name} / ${child.name}`}
                        >
                          <span class="facet-name">{child.name}</span>
                          <span class="count">{child.count.toLocaleString()}</span>
                        </button>
                      )}
                    </For>
                  </Show>
                </>
              )}
            </For>
          </section>
        </Show>

        <Show when={mode() === 'folders'}>
          <section class="side-section grow">
            <Show when={props.folders} fallback={<div class="side-empty">indexing…</div>}>
              {(root) => (
                <>
                  <button
                    class={props.folder === null ? 'folder selected' : 'folder'}
                    style={{ 'padding-left': '8px' }}
                    onClick={() => props.onFolder(null)}
                  >
                    <span class="twisty">
                      <span class="twisty-spacer" />
                    </span>
                    <span class="folder-name">All</span>
                    <span class="count">{root().count.toLocaleString()}</span>
                  </button>
                  <For each={root().children}>{(child) => renderNode(child, 0)}</For>
                </>
              )}
            </Show>
          </section>
        </Show>
      </Show>

      <section class="side-section facts-section">
        <div class="side-title">Library</div>
        <Show when={props.library} fallback={<div class="side-empty">no library open</div>}>
          {(lib) => (
            <dl class="facts">
              <dt>Root</dt>
              <dd title={lib().root}>{lib().root}</dd>
              <dt>Sounds</dt>
              <dd>{lib().count.toLocaleString()}</dd>
              <dt>Status</dt>
              <dd>
                {lib().indexing
                  ? `indexing ${lib().indexed.toLocaleString()} / ${lib().total.toLocaleString()}`
                  : 'ready'}
              </dd>
            </dl>
          )}
        </Show>
      </section>
    </aside>
  )
}
