/**
 * Whether the focused element is one that consumes typing.
 *
 * iX components put a native `<input>` inside a shadow root, so `activeElement`
 * reports the host (`IX-INPUT`, `IX-SELECT`, …), not the inner element. That is
 * why the host's tag name has to be checked as well.
 *
 * The list is deliberately narrow. Treating the whole `IX-` namespace as "in a
 * field" — which the sound table used to do — also covers `IX-BUTTON`, so after
 * clicking AUTO PLAY, Loop or the volume slider, every arrow key, Home/End,
 * PageUp/PageDown and Ctrl+A was swallowed and the table could not be navigated
 * until the user clicked back into it. Buttons do take Space and Enter, so those
 * still belong to the button; arrows and Ctrl+A do not.
 *
 * Both the app-level shortcuts and the table-level navigation read this, so the
 * two can never disagree again.
 */
const TEXT_ENTRY_HOSTS = new Set([
  'INPUT',
  'TEXTAREA',
  'SELECT',
  'IX-INPUT',
  'IX-SELECT',
  'IX-SLIDER',
  'IX-CHIP',
  'IX-DATEPICKER',
  'IX-TEXTAREA'
])

export function isTextEntry(element: Element | null): boolean {
  if (!element) return false
  const tag = element.tagName.toUpperCase()
  if (TEXT_ENTRY_HOSTS.has(tag)) return true
  return (element as HTMLElement).isContentEditable === true
}

/** The active element, as `document.activeElement` reports it. */
export function focusedElement(): Element | null {
  return typeof document === 'undefined' ? null : document.activeElement
}
