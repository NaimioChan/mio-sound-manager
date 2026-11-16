import { For, Show, createSignal } from 'solid-js'
import type { FolderNode, LibraryStatus } from './ipc'

interface Props {
  folders: FolderNode | null
  library: LibraryStatus | null
  selectedCategory: string | null
  selectedFolder: string | null
  onCategory: (value: string) => void
  onFolder: (value: string) => void
}

/**
 * Left rail: folder tree above, categories below.
 *
 * Categories are derived from the folder tree's own data here for P3, because the
 * shell does not yet have the facet counts wired to a command. The reference
 * implementation shows `category / subcategory` counts with a "favorites" row;
 * that lands with P4 when the sidebar gets its full interaction spec.
 */
export function Sidebar(props: Props) {
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set())

  const toggle = (path: string) => {
    const next = new Set(expanded())
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setExpanded(next)
  }

  const renderNode = (node: FolderNode, depth: number) => (
    <div class="folder-node">
      <button
        class={props.selectedFolder === node.path ? 'folder selected' : 'folder'}
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
      <section class="side-section">
        <div class="side-title">Folders</div>
        <Show
          when={props.folders}
          fallback={<div class="side-empty">no library</div>}
        >
          {(root) => (
            <>
              <button
                class={props.selectedFolder === null ? 'folder selected' : 'folder'}
                style={{ 'padding-left': '8px' }}
                onClick={() => {
                  props.onFolder('')
                }}
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

      <section class="side-section grow">
        <div class="side-title">Library</div>
        <Show
          when={props.library}
          fallback={<div class="side-empty">no library open</div>}
        >
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
