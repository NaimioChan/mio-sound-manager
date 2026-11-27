import { For, Show, createEffect, onCleanup, onMount } from 'solid-js'

/**
 * A floating menu at a point, closed by Escape, a click elsewhere, or a scroll.
 *
 * Hand-rolled rather than `ix-dropdown`: that component anchors to an *element*,
 * and a right-click menu has to open at the cursor — on a row that may be scrolled
 * away a frame later. The panel itself is drawn entirely from iX's menu tokens
 * (`--theme-menu--*`, `--theme-color-ghost--*`), so it looks like the design
 * system's menu while keeping viewport-coordinate positioning.
 *
 * A library would add a focus trap this window does not want: the keyboard belongs
 * to the sound table.
 */
export interface MenuItem {
  id: string
  label: string
  shortcut?: string
  disabled?: boolean
  /** Renders a separator instead of a row. */
  divider?: boolean
  /** Draws a tick in the leading slot: a menu item that toggles a visible state. */
  checked?: boolean
  onSelect?: () => void
}

interface Props {
  x: number
  y: number
  items: MenuItem[]
  onClose: () => void
}

const MENU_W = 248
/** `.menu-item`'s height in `styles.css`. iX's 40px is not what this panel draws. */
const ROW_H = 32
/** A separator: 1px plus its 4px margins, top and bottom. */
const DIVIDER_H = 9
const PAD = 8

export function ContextMenu(props: Props) {
  let el: HTMLDivElement | undefined

  /**
   * Keep the menu inside the window; a right-click near the edge would clip it.
   *
   * The height is summed rather than `items.length * ROW_H`: the column menu has a
   * separator, and counting it as a full row pushed the panel up by 23px for no
   * reason. The estimate is also clamped to the window: when the menu genuinely
   * does not fit at any offset, `.menu` scrolls, and pretending it is shorter than
   * it is would leave the last entries outside the viewport.
   */
  const position = () => {
    const height = Math.min(
      window.innerHeight - 16,
      props.items.reduce((sum, item) => sum + (item.divider ? DIVIDER_H : ROW_H), PAD * 2)
    )
    const left = Math.min(props.x, window.innerWidth - MENU_W - 8)
    const top = Math.min(props.y, window.innerHeight - height - 8)
    return { left: `${Math.max(8, left)}px`, top: `${Math.max(8, top)}px`, width: `${MENU_W}px` }
  }

  const onPointerDown = (event: PointerEvent) => {
    if (el && !el.contains(event.target as Node)) props.onClose()
  }

  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      props.onClose()
    }
  }

  onMount(() => {
    // Capture phase: a click on a row must close the menu without also selecting
    // that row, which is what the reference implementation does.
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', props.onClose)
  })
  onCleanup(() => {
    window.removeEventListener('pointerdown', onPointerDown, true)
    window.removeEventListener('keydown', onKey)
    window.removeEventListener('resize', props.onClose)
  })

  // A new position means a new menu; reset the opacity so it does not fade in
  // from the previous location.
  createEffect(() => {
    props.x
    props.y
    if (el) el.style.opacity = '1'
  })

  return (
    <div ref={el} class="menu" style={position()} role="menu">
      <For each={props.items}>
        {(item) => (
          <Show when={!item.divider} fallback={<div class="menu-divider" role="separator" />}>
            <button
              class="menu-item"
              role="menuitem"
              data-menu-id={item.id}
              disabled={item.disabled}
              onClick={() => {
                props.onClose()
                item.onSelect?.()
              }}
            >
              {/* A fixed leading slot, so labels stay on one left edge whether or
                  not a row is ticked. `menu-label` keeps its `flex: 1`. */}
              <span class="menu-check" aria-hidden="true">
                {item.checked ? '✓' : ''}
              </span>
              <span class="menu-label">{item.label}</span>
              <Show when={item.shortcut}>
                <span class="menu-shortcut">{item.shortcut}</span>
              </Show>
            </button>
          </Show>
        )}
      </For>
    </div>
  )
}
