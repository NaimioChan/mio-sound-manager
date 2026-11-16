/**
 * Inline SVG icons, sized to the 12–13px text they sit next to.
 *
 * Hand-drawn rather than pulled from an icon package: the UI has exactly four
 * glyphs, and a dependency that ships a thousand of them for four is a dependency
 * the webview has to parse on every start.
 *
 * `fill="currentColor"` so a button's colour state (`:hover`, `.on`) carries into
 * the icon without a second rule.
 */
interface IconProps {
  size?: number
}

export function PauseIcon(props: IconProps) {
  const size = () => props.size ?? 12
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 12 12"
      fill="currentColor"
      aria-hidden="true"
    >
      <rect x="2" y="1.5" width="2.6" height="9" rx="0.5" />
      <rect x="7.4" y="1.5" width="2.6" height="9" rx="0.5" />
    </svg>
  )
}

export function PlayIcon(props: IconProps) {
  const size = () => props.size ?? 12
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 12 12"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M3 1.6 10.4 6 3 10.4Z" />
    </svg>
  )
}

export function StopIcon(props: IconProps) {
  const size = () => props.size ?? 12
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 12 12"
      fill="currentColor"
      aria-hidden="true"
    >
      <rect x="2.2" y="2.2" width="7.6" height="7.6" rx="0.8" />
    </svg>
  )
}

/** Auto-play on select: a bolt, the same shape the reference uses for it. */
export function BoltIcon(props: IconProps) {
  const size = () => props.size ?? 12
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 12 12"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M6.9 0.6 2.4 6.6h2.5l-0.9 4.8 4.7-6.2H6.1Z" />
    </svg>
  )
}

/** Settings. A stroked gear, because a filled one at 12px is a smudge. */
export function GearIcon(props: IconProps) {
  const size = () => props.size ?? 12
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 8.9 4.6h.09A1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  )
}
