// The colour theme system.
//
// Every colour the UI draws comes from a Siemens iX token (`--theme-color-*`),
// and a theme is a set of overrides for those tokens. That is deliberate: the
// iX components read the same variables, so a theme change repaints the web
// components and our own CSS in one step, with no second palette to keep in sync.
//
// A theme is described by eight colours, not by two hundred tokens. The rest is
// derived: the neutral ramp from the background and the text colour, the ghost
// and component fills from the text colour with alpha, the selection fills from
// the accent. Deriving them is what keeps seven themes from drifting apart — a
// hand-written token list per theme is seven chances to write a hover state that
// does not match its base.
//
// Specificity note: iX's own token blocks are
// `[data-ix-theme=classic][data-ix-color-schema=dark]` (0,2,0). Our override
// block is `html[data-ix-theme][data-ix-color-schema]` (0,2,1), which wins
// regardless of source order — verified in the browser, not assumed.

import type { MessageKey } from './locales/en'

/** The eight colours a theme is made of. */
export interface Palette {
  /** The deepest surface: the window background. */
  bg: string
  /** The neutral the ramp is mixed toward, and the base of every text colour. */
  fg: string
  /** Interactive accent: primary buttons, selection, focus. */
  accent: string
  /** Second accent. iX uses it for the "active" state of controls (slider thumb). */
  dynamic: string
  /** Text drawn on top of the accent. */
  contrast: string
  success: string
  warning: string
  alarm: string
  info: string
  neutral: string
}

export interface Theme {
  id: string
  /**
   * Message key, not text. A theme's name is UI copy like any other label, so it
   * lives in `locales/` and is resolved by the picker; `theme.ts` owns palettes,
   * not words.
   */
  labelKey: MessageKey
  schema: 'dark' | 'light'
  /** Every built-in theme carries a palette; none falls back to iX's own. */
  palette: Palette
  /**
   * Where each step of the neutral ramp lands on the way from `bg` to `fg`,
   * overriding `RAMP`. A flat scheme needs its surfaces lifted off the window
   * background — Windows dark draws a card at `#2b2b2b` over a `#202020` window,
   * which a fixed ramp cannot hit — while an opinionated scheme like Tokyo Night
   * ships its own surfaces and reads better with the reference spacing.
   */
  ramp?: number[]
}

// ------------------------------------------------------------------ colours

type Rgb = [number, number, number]

function parse(hex: string): Rgb {
  const clean = hex.replace('#', '')
  const full =
    clean.length === 3
      ? clean
          .split('')
          .map((c) => c + c)
          .join('')
      : clean
  return [
    Number.parseInt(full.slice(0, 2), 16),
    Number.parseInt(full.slice(2, 4), 16),
    Number.parseInt(full.slice(4, 6), 16)
  ]
}

function toHex([r, g, b]: Rgb): string {
  const part = (n: number) =>
    Math.round(Math.min(255, Math.max(0, n)))
      .toString(16)
      .padStart(2, '0')
  return `#${part(r)}${part(g)}${part(b)}`
}

/** `t` of the way from `a` to `b`. */
function mix(a: string, b: string, t: number): string {
  const [ar, ag, ab] = parse(a)
  const [br, bg, bb] = parse(b)
  return toHex([ar + (br - ar) * t, ag + (bg - ag) * t, ab + (bb - ab) * t])
}

function alpha(color: string, a: number): string {
  const [r, g, b] = parse(color)
  return `rgba(${r},${g},${b},${a})`
}

/**
 * The neutral ramp, iX's `--theme-color-1` … `-8`.
 *
 * `-1` is the window background and `-8` the dimmest text; the steps in between
 * are surfaces and borders. The ratios are the ones iX's classic dark palette
 * actually lands on (its `-3` is 26% of the way from `-1` to `-8`), so a theme
 * built from them has the same contrast rhythm as the reference.
 */
const RAMP = [0, 0.16, 0.26, 0.35, 0.43, 0.5, 0.56, 0.74]

/** Turn a palette into the token overrides a theme applies. */
function buildTokens(
  p: Palette,
  schema: 'dark' | 'light',
  rampSteps: number[] = RAMP
): Record<string, string> {
  const ramp = rampSteps.map((t) => mix(p.bg, p.fg, t))
  const accentHover = mix(p.accent, '#ffffff', schema === 'dark' ? 0.22 : 0.12)
  const accentActive = mix(p.accent, '#000000', 0.12)
  const dynamicHover = mix(p.dynamic, '#ffffff', schema === 'dark' ? 0.2 : 0.12)
  const dynamicActive = mix(p.dynamic, '#000000', 0.1)

  const tokens: Record<string, string> = {
    // --- surfaces
    '--theme-color-0': 'transparent',
    '--theme-color-1': ramp[0]!,
    '--theme-color-1--hover': ramp[2]!,
    '--theme-color-1--active': ramp[1]!,
    '--theme-color-2': ramp[1]!,
    '--theme-color-3': ramp[2]!,
    '--theme-color-4': ramp[3]!,
    '--theme-color-5': ramp[4]!,
    '--theme-color-6': ramp[5]!,
    '--theme-color-7': ramp[6]!,
    '--theme-color-8': ramp[7]!,

    // --- text, as alpha of the one neutral the theme names
    // The alphas are set from measured contrast, not by eye: at 0.66 the soft text
    // came out at 3.99:1 over the rail surface in Studio Dark, under the 4.5:1
    // normal-text floor. `tools/check-themes.mjs` measures every theme against that
    // floor, so these numbers are the ones that pass rather than the ones that look
    // right.
    '--theme-color-std-text': alpha(p.fg, 0.94),
    '--theme-color-soft-text': alpha(p.fg, 0.78),
    '--theme-color-weak-text': alpha(p.fg, 0.58),
    '--theme-color-inv-std-text': alpha(p.bg, 0.92),
    '--theme-color-inv-soft-text': alpha(p.bg, 0.66),
    '--theme-color-inv-weak-text': alpha(p.bg, 0.42),
    '--theme-color-contrast-text': schema === 'dark' ? '#ffffff' : p.bg,
    '--theme-color-contrast-bdr': alpha(p.fg, 0.85),

    // --- borders
    '--theme-color-std-bdr': alpha(p.fg, 0.55),
    '--theme-color-soft-bdr': alpha(p.fg, 0.38),
    '--theme-color-weak-bdr': alpha(p.fg, 0.22),
    '--theme-color-x-weak-bdr': alpha(p.fg, 0.14),
    '--theme-color-hard-bdr': alpha(p.fg, 0.75),

    // --- the accent, in its three states
    '--theme-color-primary': p.accent,
    '--theme-color-primary--hover': accentHover,
    '--theme-color-primary--active': accentActive,
    '--theme-color-primary--disabled': alpha(p.accent, 0.45),
    '--theme-color-primary--contrast': p.contrast,
    '--theme-color-dynamic': p.dynamic,
    '--theme-color-dynamic--hover': dynamicHover,
    '--theme-color-dynamic--active': dynamicActive,
    '--theme-color-dynamic-alt': p.dynamic,
    '--theme-color-dynamic-alt--hover': dynamicHover,
    '--theme-color-dynamic-alt--active': dynamicActive,
    '--theme-color-focus-bdr': p.dynamic,
    '--theme-color-secondary': alpha(p.accent, 0.14),
    '--theme-color-secondary--hover': alpha(p.accent, 0.22),
    '--theme-color-secondary--active': alpha(p.accent, 0.3),

    // --- ghost fills: what a hovered button and a selected row are drawn with
    '--theme-color-ghost': 'transparent',
    '--theme-color-ghost--hover': alpha(p.fg, 0.1),
    '--theme-color-ghost--active': alpha(p.fg, 0.16),
    '--theme-color-ghost--selected': alpha(p.accent, 0.16),
    '--theme-color-ghost--selected-hover': alpha(p.accent, 0.22),
    '--theme-color-ghost--selected-active': alpha(p.accent, 0.28),
    '--theme-color-ghost-alt': alpha(p.fg, 0.04),
    '--theme-color-ghost-alt--hover': alpha(p.fg, 0.1),
    '--theme-color-ghost-alt--active': alpha(p.fg, 0.16),
    '--theme-color-ghost-alt--selected': alpha(p.accent, 0.14),
    '--theme-color-ghost-alt--selected-hover': alpha(p.accent, 0.2),
    '--theme-color-ghost-alt--selected-active': alpha(p.accent, 0.26),
    '--theme-color-ghost-primary--hover': alpha(p.accent, 0.14),
    '--theme-color-ghost-primary--active': alpha(p.accent, 0.2),

    // --- component fills: input backgrounds, dividers, disabled states
    '--theme-color-component-1': alpha(p.fg, 0.14),
    '--theme-color-component-1--hover': alpha(p.fg, 0.22),
    '--theme-color-component-1--active': alpha(p.fg, 0.26),
    '--theme-color-component-2': alpha(p.fg, 0.09),
    '--theme-color-component-3': alpha(p.fg, 0.24),
    '--theme-color-component-4': alpha(p.fg, 0.16),
    '--theme-color-component-5': alpha(p.fg, 0.34),
    '--theme-color-component-6': alpha(p.fg, 0.44),
    '--theme-color-component-7': alpha(p.accent, 0.16),
    '--theme-color-component-7--hover': alpha(p.accent, 0.24),
    '--theme-color-component-7--active': alpha(p.accent, 0.3),
    '--theme-color-component-8': mix(p.bg, p.fg, 0.1),
    '--theme-color-component-8--hover': mix(p.bg, p.fg, 0.15),
    '--theme-color-component-9': p.accent,
    '--theme-color-component-9--hover': accentHover,
    '--theme-color-component-9--active': accentActive,
    '--theme-color-component-9--disabled': alpha(p.accent, 0.3),
    '--theme-color-component-10': p.dynamic,
    '--theme-color-component-10--hover': dynamicHover,
    '--theme-color-component-10--active': dynamicActive,
    '--theme-color-component-10--disabled': alpha(p.dynamic, 0.3),
    '--theme-color-component-11': alpha(p.fg, 0.12),
    '--theme-color-component-error': mix(p.bg, p.alarm, 0.22),
    '--theme-color-component-info': mix(p.bg, p.info, 0.22),
    '--theme-color-component-warning': mix(p.bg, p.warning, 0.22),

    // --- semantic. `critical` is iX's own step between warning and alarm.
    '--theme-color-success': p.success,
    '--theme-color-success--hover': mix(p.success, '#ffffff', 0.18),
    '--theme-color-success--active': mix(p.success, '#000000', 0.12),
    '--theme-color-success--contrast': p.contrast,
    '--theme-color-success-40': alpha(p.success, 0.4),
    '--theme-color-warning': p.warning,
    '--theme-color-warning--hover': mix(p.warning, '#ffffff', 0.18),
    '--theme-color-warning--active': mix(p.warning, '#000000', 0.12),
    '--theme-color-warning--contrast': p.contrast,
    '--theme-color-warning-bdr': p.warning,
    '--theme-color-warning-text': p.warning,
    '--theme-color-warning-10': alpha(p.warning, 0.1),
    '--theme-color-warning-40': alpha(p.warning, 0.4),
    '--theme-color-alarm': p.alarm,
    '--theme-color-alarm--hover': mix(p.alarm, '#ffffff', 0.18),
    '--theme-color-alarm--active': mix(p.alarm, '#000000', 0.12),
    '--theme-color-alarm--contrast': p.contrast,
    '--theme-color-alarm-bdr': p.alarm,
    '--theme-color-alarm-text': mix(p.alarm, '#ffffff', 0.28),
    '--theme-color-alarm-10': alpha(p.alarm, 0.1),
    '--theme-color-alarm-40': alpha(p.alarm, 0.4),
    '--theme-color-critical': mix(p.warning, p.alarm, 0.5),
    '--theme-color-critical--hover': mix(mix(p.warning, p.alarm, 0.5), '#ffffff', 0.18),
    '--theme-color-critical--active': mix(mix(p.warning, p.alarm, 0.5), '#000000', 0.12),
    '--theme-color-critical--contrast': p.contrast,
    '--theme-color-critical-40': alpha(mix(p.warning, p.alarm, 0.5), 0.4),
    '--theme-color-info': p.info,
    '--theme-color-info--hover': mix(p.info, '#ffffff', 0.18),
    '--theme-color-info--active': mix(p.info, '#000000', 0.12),
    '--theme-color-info--contrast': p.contrast,
    '--theme-color-info-40': alpha(p.info, 0.4),
    '--theme-color-neutral': p.neutral,
    '--theme-color-neutral--hover': mix(p.neutral, '#ffffff', 0.12),
    '--theme-color-neutral--active': mix(p.neutral, '#000000', 0.1),
    '--theme-color-neutral--contrast': p.contrast,
    '--theme-color-neutral-40': alpha(p.neutral, 0.4),

    // --- overlays
    '--theme-color-backdrop': schema === 'dark' ? 'rgba(0,0,0,0.62)' : 'rgba(18,18,26,0.38)',
    '--theme-color-backdrop-3': schema === 'dark' ? 'rgba(0,0,0,0.8)' : 'rgba(18,18,26,0.5)',
    '--theme-color-shadow-1': schema === 'dark' ? 'rgba(0,0,0,0.6)' : 'rgba(20,20,30,0.18)',
    '--theme-color-shadow-2': schema === 'dark' ? '#000000' : 'rgba(20,20,30,0.22)',
    '--theme-color-shadow-3': schema === 'dark' ? 'rgba(0,0,0,0.6)' : 'rgba(20,20,30,0.2)',

    // --- the scrollbar follows the ramp, so a themed app does not keep grey bars
    '--theme-scrollbar-thumb--background': alpha(p.fg, 0.26),
    '--theme-scrollbar-thumb--background--hover': alpha(p.fg, 0.42),
    '--theme-scrollbar-track--background': 'transparent',

    // --- the waveform's own colours, derived so they track the accent.
    //
    // The selection is drawn by scrimming what is *outside* it and outlining it,
    // which is what makes it readable over a wall of bars: tinting the inside
    // fails exactly when the waveform is loud, because the bars are already the
    // accent's colour family. The scrim is the surface colour, so it darkens a
    // dark theme and lightens a light one — the same gesture in both.
    '--mio-wave': p.dynamic,
    '--mio-region-scrim': alpha(p.bg, 0.55),
    '--mio-region-fill': alpha(p.accent, 0.1),
    '--mio-region-edge': mix(p.accent, schema === 'dark' ? '#ffffff' : '#000000', 0.35),
    '--mio-playhead': mix(p.fg, '#ffffff', 0.5)
  }

  // A light theme's shadows have to be softer than a dark one's, or every raised
  // surface gets a black outline that reads as a border.
  if (schema === 'light') {
    tokens['--theme-shadow-1'] = '0 1px 2px 0 rgba(20,20,30,0.14), 0 1px 1px 0 rgba(20,20,30,0.08)'
    tokens['--theme-shadow-3'] = '0 2px 6px 0 rgba(20,20,30,0.12), 0 0 8px 0 rgba(20,20,30,0.08)'
    tokens['--theme-shadow-4'] =
      '0 0 2px 0 rgba(20,20,30,0.1), 0 4px 8px 0 rgba(20,20,30,0.12), 0 12px 18px 0 rgba(20,20,30,0.1)'
  }

  return tokens
}

// ------------------------------------------------------------------- themes

/**
 * The built-in themes.
 *
 * `windows-*` are the pair the app opens in: Windows 11's own Fluent scheme, so
 * an app running next to Explorer and Settings does not look like it came from
 * another desktop. `studio-*`, Amber and Frost are alternatives in the same
 * layout — a theme changes the light, not the UI.
 *
 * The two Windows ramps are flatter than `RAMP`: Fluent keeps its window at
 * `#202020`/`#f3f3f3` and lifts cards only a few percent, where a fixed ramp
 * would put `--theme-color-2` at a much lighter grey. The steps were read off
 * WinUI's own base/layer/card fills rather than guessed.
 */
export const THEMES: Theme[] = [
  {
    id: 'windows-dark',
    labelKey: 'theme.windowsDark',
    schema: 'dark',
    ramp: [0, 0.06, 0.11, 0.17, 0.25, 0.35, 0.46, 0.74],
    palette: {
      bg: '#202020',
      fg: '#f2f2f2',
      // Fluent's `AccentFillColorDefault` in dark mode: the Light2 tint of the
      // system accent, with near-black text on it.
      accent: '#4cc2ff',
      dynamic: '#99ebff',
      contrast: '#0a1a24',
      success: '#6ccb5f',
      warning: '#fce100',
      alarm: '#ff99a4',
      info: '#4cc2ff',
      neutral: '#a0a0a0'
    }
  },
  {
    id: 'windows-light',
    labelKey: 'theme.windowsLight',
    schema: 'light',
    ramp: [0, 0.055, 0.1, 0.16, 0.24, 0.33, 0.44, 0.74],
    palette: {
      bg: '#f3f3f3',
      fg: '#1a1a1a',
      // `AccentFillColorDefault` light: the Dark1 tint, with white text on it.
      accent: '#0067c0',
      dynamic: '#0078d4',
      contrast: '#ffffff',
      success: '#0f7b0f',
      warning: '#9d5d00',
      alarm: '#c42b1c',
      info: '#0067c0',
      neutral: '#5d5d5d'
    }
  },
  {
    id: 'studio-dark',
    labelKey: 'theme.studioDark',
    schema: 'dark',
    palette: {
      bg: '#101014',
      fg: '#b9b6cc',
      accent: '#a78bfa',
      dynamic: '#a78bfa',
      contrast: '#0d0b14',
      success: '#4bbf87',
      warning: '#e0a33a',
      alarm: '#f0576b',
      info: '#5b9df5',
      neutral: '#b6b3c2'
    }
  },
  {
    id: 'studio-light',
    labelKey: 'theme.studioLight',
    schema: 'light',
    palette: {
      bg: '#ffffff',
      fg: '#3a3a4a',
      accent: '#5b5bd6',
      dynamic: '#4f4fd0',
      contrast: '#ffffff',
      success: '#1f8f5f',
      warning: '#a8690a',
      alarm: '#c22a42',
      info: '#2264c8',
      neutral: '#6f6f80'
    }
  },
  {
    id: 'amber-console',
    labelKey: 'theme.amber',
    schema: 'dark',
    palette: {
      bg: '#0d0b08',
      fg: '#b8a894',
      accent: '#f0a02a',
      dynamic: '#f0a02a',
      contrast: '#14100a',
      success: '#5fb06a',
      warning: '#f0a02a',
      alarm: '#e0563f',
      info: '#4f9fd0',
      neutral: '#b8a894'
    }
  },
  {
    id: 'frost-dark',
    labelKey: 'theme.frost',
    schema: 'dark',
    palette: {
      bg: '#12171d',
      fg: '#a9bcc9',
      accent: '#88c0d0',
      dynamic: '#8fbcbb',
      contrast: '#0d1116',
      success: '#a3be8c',
      warning: '#ebcb8b',
      alarm: '#bf616a',
      info: '#81a1c1',
      neutral: '#a9bcc9'
    }
  },
  {
    id: 'tokyo-night',
    labelKey: 'theme.tokyoNight',
    schema: 'dark',
    palette: {
      // The stock Tokyo Night "night" background and foreground. The surfaces
      // land on its own `bg_dark` `#16161e` and `bg_highlight` `#292e42` with the
      // reference ramp, so this one keeps `RAMP` untouched.
      bg: '#1a1b26',
      fg: '#c0caf5',
      accent: '#7aa2f7',
      dynamic: '#bb9af7',
      contrast: '#15161e',
      success: '#9ece6a',
      warning: '#e0af68',
      alarm: '#f7768e',
      info: '#7dcfff',
      neutral: '#a9b1d6'
    }
  }
]

export const DEFAULT_THEME = THEMES[0]!.id

const STORAGE_KEY = 'mio:theme'
/** The schema, stored separately so `index.html` can theme the first paint. */
const SCHEMA_KEY = 'mio:theme-schema'
const STYLE_ID = 'mio-theme'

export function findTheme(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0]!
}

export function loadThemeId(): string {
  const stored = localStorage.getItem(STORAGE_KEY)
  return stored && THEMES.some((t) => t.id === stored) ? stored : DEFAULT_THEME
}

/**
 * Apply a theme to the document.
 *
 * The overrides go into one `<style>` element that is replaced, not appended to:
 * seven themes' worth of stale token blocks would leave the last one winning by
 * accident rather than by choice.
 */
export function applyTheme(id: string): Theme {
  const theme = findTheme(id)
  const root = document.documentElement
  root.dataset.ixTheme = 'classic'
  root.dataset.ixColorSchema = theme.schema
  root.dataset.mioTheme = theme.id
  // `color-scheme` drives the native scrollbar and the caret, which do not read
  // CSS variables.
  root.style.colorScheme = theme.schema

  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = STYLE_ID
    document.head.appendChild(style)
  }
  const body = Object.entries(buildTokens(theme.palette, theme.schema, theme.ramp))
    .map(([name, value]) => `${name}:${value};`)
    .join('')
  style.textContent = `html[data-ix-theme][data-ix-color-schema]{${body}}`

  localStorage.setItem(STORAGE_KEY, theme.id)
  localStorage.setItem(SCHEMA_KEY, theme.schema)
  return theme
}

/**
 * Debug hook for the CDP checks.
 *
 * `tools/check-themes.mjs` drives the real window, and the settings dialog's theme
 * picker cannot be clicked through a modal from there without also opening the
 * dialog. Exposing the one function the dialog calls means the check exercises the
 * same code path instead of reimplementing it.
 */
declare global {
  interface Window {
    __mioApplyTheme?: (id: string) => string
  }
}

export function installThemeDebugHook(): void {
  window.__mioApplyTheme = (id: string) => {
    applyTheme(id)
    return 'ok'
  }
}
