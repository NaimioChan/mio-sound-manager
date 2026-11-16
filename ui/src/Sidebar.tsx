import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import type { Facet, FolderNode, LibraryStatus } from './ipc'

/**
 * The left rail: one collapsible group per open library, and inside each either
 * categories or folders.
 *
 * Several libraries at once is the point. A sample collection is usually split
 * across packs on different drives, and switching between them should not mean
 * reopening and reindexing — so the rail lists every open library as a section,
 * the active one is highlighted, and clicking a header switches to it.
 *
 * The header shows the folder *name*, not the path: a 232px rail cannot hold
 * `D:\Samples\Bought\Epic Stock Media 202GB\052 Epic.Stock.Media.Card.Game`, and
 * the distinguishing part of a library path is its last segment. The full path is
 * the tooltip and the settings dialog.
 */
type Mode = 'categories' | 'folders'

const MODE_KEY = 'reslip:sidebar-mode'
/** Expanded folders, remembered per library root. */
const EXPANDED_PREFIX = 'reslip:folders-expanded:'

function loadExpanded(root: string | null): Set<string> {
  if (!root) return new Set()
  try {
    const raw = localStorage.getItem(EXPANDED_PREFIX + root)
    const parsed = raw ? JSON.parse(raw) : null
    if (Array.isArray(parsed)) return new Set(parsed.filter((p) => typeof p === 'string'))
  } catch {
    // A corrupt entry costs one collapsed tree, not a broken rail.
  }
  return new Set()
}

function saveExpanded(root: string | null, expanded: Set<string>): void {
  if (!root) return
  try {
    localStorage.setItem(EXPANDED_PREFIX + root, JSON.stringify([...expanded]))
  } catch {
    // Quota or a locked profile: the tree still works this session.
  }
}

interface Props {
  libraries: LibraryStatus[]
  facets: Facet[]
  favorites: number
  total: number
  folders: FolderNode | null
  category: string | null
  subcategory: string | null
  folder: string | null
  favoritesOnly: boolean
  onActivate: (root: string) => void
  onCategory: (category: string | null, subcategory: string | null) => void
  onFolder: (folder: string | null) => void
  onToggleFavorites: () => void
  onOpenSettings: () => void
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
  const [expandedCats, setExpandedCats] = createSignal<Set<string>>(new Set())
  /**
   * Which library groups are collapsed. The active library always expands: a rail
   * where the library you are looking at is folded shut is a rail that hides its
   * own contents.
   */
  const [collapsedLibs, setCollapsedLibs] = createSignal<Set<string>>(new Set())
  /**
   * Folders whose children are open, by path.
   *
   * The tree is rebuilt from the index on every library switch, so this is keyed
   * by path and reloaded from `localStorage` per root. Without it, a deep pack
   * re-collapses every time the rail re-renders, and a 200GB library cannot be
   * browsed at all: every click on a child folder would be one level deep.
   */
  const activeRoot = createMemo(() => props.libraries.find((l) => l.active)?.root ?? null)
  const [expandedFolders, setExpandedFolders] = createSignal<Set<string>>(
    loadExpanded(props.libraries.find((l) => l.active)?.root ?? null)
  )

  // Switching libraries swaps the tree, so the open set has to swap with it.
  let loadedFor: string | null = props.libraries.find((l) => l.active)?.root ?? null
  createEffect(() => {
    const root = activeRoot()
    if (root === loadedFor) return
    loadedFor = root
    setExpandedFolders(loadExpanded(root))
  })

  const toggleFolder = (path: string) => {
    const next = new Set(expandedFolders())
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setExpandedFolders(next)
    saveExpanded(activeRoot(), next)
  }


  const changeMode = (next: Mode) => {
    setMode(next)
    localStorage.setItem(MODE_KEY, next)
  }

  const toggleCat = (name: string) => {
    const next = new Set(expandedCats())
    if (next.has(name)) next.delete(name)
    else next.add(name)
    setExpandedCats(next)
  }

  const toggleLib = (root: string) => {
    const next = new Set(collapsedLibs())
    if (next.has(root)) next.delete(root)
    else next.add(root)
    setCollapsedLibs(next)
  }

  const isOpen = (library: LibraryStatus) =>
    library.active || !collapsedLibs().has(library.root)

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

  const renderFolder = (node: FolderNode, depth: number) => {
    const hasChildren = () => node.children.length > 0
    const isOpen = () => expandedFolders().has(node.path)
    return (
      <div class="folder-node">
        <div
          class={props.folder === node.path ? 'folder selected' : 'folder'}
          style={{ 'padding-left': `${4 + depth * 12}px` }}
          title={node.path}
          onClick={() => props.onFolder(props.folder === node.path ? null : node.path)}
          // Double-click toggles, the way the reference implementation does it;
          // the twisty is the discoverable affordance, this is the fast one.
          onDblClick={() => hasChildren() && toggleFolder(node.path)}
        >
          <span
            class={hasChildren() ? 'twisty' : 'twisty spacer'}
            onClick={(e) => {
              // The row's click selects the folder; the twisty must not also do it.
              e.stopPropagation()
              if (hasChildren()) toggleFolder(node.path)
            }}
            title={hasChildren() ? (isOpen() ? 'Collapse' : 'Expand') : undefined}
          >
            {hasChildren() ? (isOpen() ? '▾' : '▸') : ''}
          </span>
          <span class="folder-name">{node.name}</span>
          <span class="count">{node.count.toLocaleString()}</span>
        </div>
        {/*
          Children are mounted only while the folder is open. Rendering the whole
          tree and hiding it with CSS would build thousands of nodes for a real
          sample library (271k files) on every rail re-render.
        */}
        <Show when={isOpen() && hasChildren()}>
          <For each={node.children}>{(child) => renderFolder(child, depth + 1)}</For>
        </Show>
      </div>
    )
  }

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
        <button
          class="side-mode gear"
          onClick={props.onOpenSettings}
          title="Settings"
          aria-label="Settings"
        >
          ⚙
        </button>
      </div>

      <Show
        when={props.libraries.length > 0}
        fallback={<div class="side-empty">no library open</div>}
      >
        <section class="side-section grow">
          <For each={props.libraries}>
            {(library) => (
              <div class={library.active ? 'lib-group active' : 'lib-group'}>
                <button
                  class="lib-head"
                  onClick={() => props.onActivate(library.root)}
                  title={library.root}
                >
                  <span
                    class="twisty"
                    onClick={(e) => {
                      e.stopPropagation()
                      toggleLib(library.root)
                    }}
                  >
                    {isOpen(library) ? '▾' : '▸'}
                  </span>
                  <span class="lib-name">{library.name}</span>
                  <span class="count">{library.count.toLocaleString()}</span>
                </button>

                <Show when={isOpen(library)}>
                  <Show when={library.active}>
                    <Show when={mode() === 'categories'}>
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
                          props.category === null && !props.favoritesOnly
                            ? 'facet selected'
                            : 'facet'
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
                                  toggleCat(group.name)
                                }}
                              >
                                <Show
                                  when={group.children.length > 0}
                                  fallback={<span class="twisty-spacer" />}
                                >
                                  {expandedCats().has(group.name) ? '▾' : '▸'}
                                </Show>
                              </span>
                              <span class="facet-name">{group.name}</span>
                              <span class="count">{group.count.toLocaleString()}</span>
                            </button>

                            <Show when={expandedCats().has(group.name)}>
                              <For each={group.children}>
                                {(child) => (
                                  <button
                                    class={
                                      props.category === group.name &&
                                      props.subcategory === child.name
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
                    </Show>

                    <Show when={mode() === 'folders'}>
                      {/* The empty twisty slot keeps this row's name on the same
                          left edge as the folder rows below it. Without it the
                          root sits 16px to the left of every child, which reads
                          as a different kind of thing rather than the top of the
                          same tree. */}
                      <button
                        class={props.folder === null ? 'folder selected' : 'folder'}
                        style={{ 'padding-left': '4px' }}
                        onClick={() => props.onFolder(null)}
                      >
                        <span class="twisty spacer" />
                        <span class="folder-name">All folders</span>
                        <span class="count">{library.count.toLocaleString()}</span>
                      </button>
                      <Show when={props.folders} fallback={<div class="side-empty">…</div>}>
                        {(root) => (
                          <For each={root().children}>
                            {(child) => renderFolder(child, 0)}
                          </For>
                        )}
                      </Show>
                    </Show>
                  </Show>

                  {/*
                    An inactive library shows only its header. Its facet counts
                    belong to a different database, and drawing them here would
                    mean either a second query per library or a number that
                    silently belongs to the active one.
                  */}
                  <Show when={!library.active}>
                    <div class="lib-inactive dim">click to switch</div>
                  </Show>
                </Show>
              </div>
            )}
          </For>
        </section>
      </Show>
    </aside>
  )
}
