import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import { UNCATEGORIZED, categoryName, t } from './i18n'
import type {
  Facet,
  FolderMenuRequest,
  FolderNode,
  LibraryStatus,
  UcsCategory,
  UcsFacet
} from './ipc'
import { ICON } from './icons'

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
 *
 * Category and subcategory rows are multi-select. A plain click replaces the
 * selection, Ctrl+click adds or removes one row, and a click on the only lit row
 * clears the filter. The index ORs categories and intersects them with
 * subcategories, so several lit rows are one query.
 */
export type SidebarMode = 'categories' | 'ucs' | 'folders'

/** Expanded folders, remembered per library root. */
const EXPANDED_PREFIX = 'mio:folders-expanded:'

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
  ucsFacets: UcsFacet[]
  ucsCatalog: UcsCategory[]
  favorites: number
  total: number
  folders: FolderNode | null
  /** Lit category rows. */
  categories: string[]
  /** Lit subcategory rows, each remembering its parent category. */
  subcategories: { category: string; sub: string }[]
  ucsCategories: string[]
  ucsSubcategories: { category: string; sub: string }[]
  ucsCatIds: string[]
  /**
   * Which facet tree the rail shows. Owned by `App` so that switching modes and
   * clearing the filters that mode applied happen in one place; the rail only
   * renders it.
   */
  mode: SidebarMode
  folder: string | null
  favoritesOnly: boolean
  onActivate: (root: string) => void
  /** `additive` is true when Ctrl (or Cmd) was held. */
  onCategory: (category: string | null, additive: boolean) => void
  onSubcategory: (category: string, subcategory: string, additive: boolean) => void
  onUcsCategory: (category: string | null, additive: boolean) => void
  onUcsSubcategory: (category: string, subcategory: string, additive: boolean, catId?: string) => void
  onMode: (mode: SidebarMode) => void
  onFolder: (folder: string | null) => void
  onToggleFavorites: () => void
  onOpenSettings: () => void
  /** Right-click on a library header: the per-library management menu. */
  onLibraryMenu: (library: LibraryStatus, x: number, y: number) => void
  /** Right-click on a folder row: reveal, filter, copy path. */
  onFolderMenu: (request: FolderMenuRequest) => void
}

/** One category with its subcategories, aggregated from the flat facet list. */
interface CategoryGroup {
  name: string
  count: number
  children: { name: string; count: number; filterName?: string; catId?: string }[]
}

/** Ctrl on Windows and Linux, Cmd elsewhere. Matches the table's selection. */
function additiveOf(event: MouseEvent): boolean {
  return event.ctrlKey || event.metaKey
}

export function Sidebar(props: Props) {
  const [expandedCats, setExpandedCats] = createSignal<Set<string>>(new Set())
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

  const changeMode = (next: SidebarMode) => {
    props.onMode(next)
    // `keyboard.ts` counts IX-SELECT as a text field, so after picking a mode the
    // inner input keeps focus and the arrow keys move a text caret instead of the
    // list. Handing focus back to the body makes the table navigable again; the
    // select must stay in that set or typing into its filter would steer the table.
    ;(document.activeElement as HTMLElement | null)?.blur()
  }

  const toggleCat = (name: string) => {
    const next = new Set(expandedCats())
    if (next.has(name)) next.delete(name)
    else next.add(name)
    setExpandedCats(next)
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
      if (a.name === UNCATEGORIZED) return 1
      if (b.name === UNCATEGORIZED) return -1
      return b.count - a.count
    })
    return list
  })

  const ucsGroups = createMemo<CategoryGroup[]>(() => {
    const byName = new Map<string, CategoryGroup>()
    for (const facet of props.ucsFacets) {
      let group = byName.get(facet.category)
      if (!group) {
        group = { name: facet.category, count: 0, children: [] }
        byName.set(facet.category, group)
      }
      group.count += facet.count
      if (facet.subcategory) group.children.push({ name: facet.subcategory, count: facet.count })
    }
    return [...byName.values()]
      .map((group) => ({ ...group, children: group.children.sort((a, b) => b.count - a.count) }))
      .sort((a, b) => b.count - a.count)
  })

  /**
   * The catalog is the stable UCS tree; facet counts are the live index counts.
   * Children with no indexed rows stay hidden, so the rail does not become a
   * 753-row wall on a library that only uses a small part of UCS.
   */
  const ucsCatalogGroups = createMemo<CategoryGroup[]>(() => {
    const parentCounts = new Map<string, number>()
    for (const facet of props.ucsFacets) {
      parentCounts.set(facet.category, (parentCounts.get(facet.category) ?? 0) + facet.count)
    }
    const byName = new Map<string, CategoryGroup>()
    for (const entry of props.ucsCatalog) {
      const count = entry.count
      let group = byName.get(entry.category)
      if (!group) {
        group = { name: entry.category, count: parentCounts.get(entry.category) ?? 0, children: [] }
        byName.set(entry.category, group)
      }
      if (count > 0) {
        group.children.push({
          name: `${entry.catId}${entry.subcategory ? ` · ${entry.subcategory}` : ''}`,
          count,
          filterName: entry.subcategory ?? '',
          catId: entry.catId
        })
      }
    }
    return [...byName.values()]
      .filter((group) => group.count > 0)
      .map((group) => ({ ...group, children: group.children.sort((a, b) => b.count - a.count) }))
      .sort((a, b) => b.count - a.count)
  })

  const categoryLit = (name: string) => props.categories.includes(name)
  const subcategoryLit = (category: string, sub: string) =>
    props.subcategories.some((entry) => entry.category === category && entry.sub === sub)
  const ucsCategoryLit = (name: string) => props.ucsCategories.includes(name)
  const ucsSubcategoryLit = (category: string, sub: string) =>
    props.ucsSubcategories.some((entry) => entry.category === category && entry.sub === sub)

  const renderFolder = (node: FolderNode, depth: number) => {
    const hasChildren = () => node.children.length > 0
    const open = () => expandedFolders().has(node.path)
    return (
      <div class="folder-node">
        <div
          class={props.folder === node.path ? 'folder selected' : 'folder'}
          data-folder={node.path}
          style={{ 'padding-left': `${4 + depth * 12}px` }}
          title={node.path}
          onClick={() => props.onFolder(props.folder === node.path ? null : node.path)}
          // Double-click toggles, the way the reference implementation does it;
          // the twisty is the discoverable affordance, this is the fast one.
          onDblClick={() => hasChildren() && toggleFolder(node.path)}
          // The same hand-drawn menu the sound rows use. A native menu here was
          // the one place the window fell back to Explorer's default panel, which
          // does not follow the theme and cannot carry an action of ours.
          onContextMenu={(event) => {
            event.preventDefault()
            props.onFolderMenu({
              path: node.path,
              name: node.name,
              x: event.clientX,
              y: event.clientY
            })
          }}
        >
          <span
            class={hasChildren() ? 'twisty' : 'twisty spacer'}
            onClick={(e) => {
              // The row's click selects the folder; the twisty must not also do it.
              e.stopPropagation()
              if (hasChildren()) toggleFolder(node.path)
            }}
            title={hasChildren() ? (open() ? t('common.collapse') : t('common.expand')) : undefined}
          >
            {hasChildren() ? (open() ? '▾' : '▸') : ''}
          </span>
          <span class="folder-name">{node.name}</span>
          <span class="count">{node.count.toLocaleString()}</span>
        </div>
        {/*
          Children are mounted only while the folder is open. Rendering the whole
          tree and hiding it with CSS would build thousands of nodes for a real
          sample library (271k files) on every rail re-render.
        */}
        <Show when={open() && hasChildren()}>
          <For each={node.children}>{(child) => renderFolder(child, depth + 1)}</For>
        </Show>
      </div>
    )
  }

  return (
    <aside class="sidebar" data-sidebar-mode={props.mode}>
      <div class="side-modes">
        <ix-select
          id="sidebar-mode"
          value={props.mode}
          allowClear={false}
          aria-label={t('side.view')}
          on:valueChange={(e: CustomEvent<string>) => changeMode(e.detail as SidebarMode)}
        >
          <ix-select-item value="categories" label={t('side.categories')} />
          <ix-select-item value="ucs" label={t('side.ucs')} />
          <ix-select-item value="folders" label={t('side.folders')} />
        </ix-select>
        <button
          class="side-gear"
          onClick={props.onOpenSettings}
          title={t('topbar.settings')}
          aria-label={t('topbar.settingsLabel')}
        >
          <ix-icon name={ICON.settings} size="16" />
        </button>
      </div>

      <Show
        when={props.libraries.length > 0}
        fallback={<div class="side-empty">{t('side.noLibrary')}</div>}
      >
        <section class="side-section grow">
          <For each={props.libraries}>
            {(library) => (
              <div class={library.active ? 'lib-group active' : 'lib-group'}>
                <button
                  class="lib-head"
                  onClick={() => props.onActivate(library.root)}
                  onContextMenu={(e) => {
                    // The management menu belongs to the library, not to the
                    // window: reindex and close act on one library while several
                    // are open, which is why they are not in Settings.
                    e.preventDefault()
                    props.onLibraryMenu(library, e.clientX, e.clientY)
                  }}
                  title={library.root}
                >
                  <span class="twisty spacer" aria-hidden="true" />
                  <span class="lib-name">{library.name}</span>
                  <span class="count">{library.count.toLocaleString()}</span>
                </button>

                <Show when={library.active}>
                  <Show when={props.mode === 'categories' || props.mode === 'ucs'}>
                    <button
                      class={props.favoritesOnly ? 'facet selected' : 'facet'}
                      onClick={props.onToggleFavorites}
                      title={t('side.favoritesOnly')}
                    >
                      <span class="facet-name">★ {t('side.favorites')}</span>
                      <span class="count">{props.favorites.toLocaleString()}</span>
                    </button>
                    <button
                      class={
                        (props.mode === 'ucs' ? props.ucsCategories.length === 0 : props.categories.length === 0) &&
                        !props.favoritesOnly
                          ? 'facet selected'
                          : 'facet'
                      }
                      data-category=""
                      onClick={() =>
                         props.mode === 'ucs'
                           ? props.onUcsCategory(null, false)
                           : props.onCategory(null, false)}
                      title={t('side.allSounds')}
                    >
                      <span class="facet-name">{t('side.allSounds')}</span>
                      <span class="count">{props.total.toLocaleString()}</span>
                    </button>

                    <For each={props.mode === 'ucs' ? (props.ucsCatalog.length > 0 ? ucsCatalogGroups() : ucsGroups()) : groups()}>
                      {(group) => (
                        <>
                          <button
                            class={(props.mode === 'ucs' ? ucsCategoryLit(group.name) : categoryLit(group.name)) ? 'facet selected' : 'facet'}
                            data-category={group.name}
                            onClick={(event) =>
                              props.mode === 'ucs'
                                ? props.onUcsCategory(group.name, additiveOf(event))
                                : props.onCategory(group.name, additiveOf(event))}
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
                            <span class="facet-name">{categoryName(group.name)}</span>
                            <span class="count">{group.count.toLocaleString()}</span>
                          </button>

                          <Show when={expandedCats().has(group.name)}>
                            <For each={group.children}>
                              {(child) => (
                                <button
                                  class={
                                    (props.mode === 'ucs'
                                       ? ucsSubcategoryLit(group.name, child.filterName ?? child.name)
                                       : subcategoryLit(group.name, child.name))
                                      ? 'facet sub selected'
                                      : 'facet sub'
                                  }
                                  data-category={group.name}
                                  data-subcategory={child.filterName ?? child.name}
                                  data-ucs-cat-id={child.catId}
                                  onClick={(event) =>
                                     props.mode === 'ucs'
                                        ? props.onUcsSubcategory(group.name, child.filterName ?? child.name, additiveOf(event), child.catId)
                                       : props.onSubcategory(group.name, child.name, additiveOf(event))}
                                  title={`${group.name} / ${child.name}`}
                                >
                                  <span class="facet-name">{categoryName(child.name)}</span>
                                  <span class="count">{child.count.toLocaleString()}</span>
                                </button>
                              )}
                            </For>
                          </Show>
                        </>
                      )}
                    </For>
                  </Show>

                  <Show when={props.mode === 'folders'}>
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
                      <span class="folder-name">{t('side.allFolders')}</span>
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

                  {/*
                    An inactive library shows only its header. Its facet counts
                    belong to a different database, and drawing them here would
                    mean either a second query per library or a number that
                    silently belongs to the active one.
                  */}
                </Show>
              </div>
            )}
          </For>
        </section>
      </Show>
    </aside>
  )
}
