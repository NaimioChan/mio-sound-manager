/**
 * Interface scale.
 *
 * The webview's own zoom factor, not a CSS transform: WebView2's `SetZoomFactor`
 * rescales layout, images and text together, so hit targets, the waveform canvas
 * and iX's shadow-DOM controls all stay consistent. A CSS `zoom` or a `rem`
 * multiplier would leave the canvas at device pixels and the px-based table
 * geometry behind.
 *
 * Tauri gates this behind `core:webview:allow-set-webview-zoom`, which is not part
 * of `core:webview:default` — it is listed in `capabilities/default.json`.
 *
 * The value is stored as a percentage so the settings picker and the stored value
 * are the same number; the factor is derived at the edge.
 */
import { getCurrentWebview } from '@tauri-apps/api/webview'

const STORAGE_KEY = 'mio:zoom'

/** The picker's steps. 80% fits a long list on a laptop, 150% on a HiDPI display. */
export const ZOOM_STEPS = [80, 90, 100, 110, 125, 150] as const

export const DEFAULT_ZOOM = 100

/** The stored scale, clamped to a step we actually offer. */
export function loadZoom(): number {
  try {
    const stored = Number(localStorage.getItem(STORAGE_KEY))
    if (ZOOM_STEPS.includes(stored as (typeof ZOOM_STEPS)[number])) return stored
  } catch {
    // A locked profile costs the preference, not the session.
  }
  return DEFAULT_ZOOM
}

/**
 * Apply a scale and remember it.
 *
 * Resolves to the scale that is actually in effect, so the picker can mirror the
 * webview rather than its own optimistic state: on the first frame of a reload the
 * element may not have a webview to talk to yet, and a silent failure would
 * otherwise leave the select showing a value the window does not have.
 */
export async function applyZoom(percent: number): Promise<number> {
  try {
    await getCurrentWebview().setZoom(percent / 100)
    localStorage.setItem(STORAGE_KEY, String(percent))
    return percent
  } catch {
    return loadZoom()
  }
}

/**
 * Debug hook for the CDP checks.
 *
 * `tools/check-tags.mjs` has to prove the window really scales, and the only
 * honest measure is `innerWidth` at two factors — driving the select through the
 * modal from outside would also close the dialog the rest of the script is
 * measuring. This exposes the same function the dialog calls.
 */
declare global {
  interface Window {
    __mioApplyZoom?: (percent: number) => Promise<number>
  }
}

export function installZoomDebugHook(): void {
  window.__mioApplyZoom = applyZoom
}
