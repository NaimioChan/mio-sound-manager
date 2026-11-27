// Verify the theme system: every built-in theme applies, repaints the iX tokens,
// and survives a reload.
//
//   node tools/check-themes.mjs [port]
//
// Reads the tokens out of the live document rather than trusting the source: a
// theme whose palette never reached the DOM would still look correct in `theme.ts`.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}
await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })

const problems = []
const results = []

// The default first: the head script has to apply it before the bundle loads, and
// the check ends by leaving the window in it.
const THEMES = [
  'windows-dark',
  'windows-light',
  'studio-dark',
  'studio-light',
  'amber-console',
  'frost-dark',
  'tokyo-night'
]
const DEFAULT_THEME = 'windows-dark'

/** Read the tokens a theme is supposed to change, plus what actually painted. */
const probe = () =>
  page.evaluate(() => {
    const cs = getComputedStyle(document.documentElement)
    const token = (name) => cs.getPropertyValue(name).trim()
    const body = getComputedStyle(document.body)
    const row = document.querySelector('.row')
    const dock = document.querySelector('.dock')
    return {
      applied: document.documentElement.dataset.mioTheme ?? 'none',
      schema: document.documentElement.dataset.ixColorSchema ?? 'none',
      colorScheme: document.documentElement.style.colorScheme,
      bg: token('--theme-color-1'),
      surface: token('--theme-color-2'),
      border: token('--theme-color-3'),
      text: token('--theme-color-std-text'),
      accent: token('--theme-color-primary'),
      dynamic: token('--theme-color-dynamic'),
      bodyBg: body.backgroundColor,
      bodyColor: body.color,
      dockBg: dock ? getComputedStyle(dock).backgroundColor : 'none',
      rowColor: row ? getComputedStyle(row).color : 'none',
      // Every theme carries a palette now, so the block must always be there; its
      // presence is the difference between "the tokens were overridden" and "iX's
      // own palette happened to be on screen".
      overrideBlock: !!document.getElementById('mio-theme'),
      overrideLength: (document.getElementById('mio-theme')?.textContent ?? '').length,
      stored: localStorage.getItem('mio:theme')
    }
  })

/** Apply a theme the way the settings dialog does: through the app's own code. */
const apply = (id) =>
  page.evaluate((id) => {
    // The settings dialog writes through `localStorage` + `applyTheme`. Reaching
    // the module directly is not possible from here, so the same effect is
    // produced by dispatching what the dialog does: set the storage key, then let
    // the app's theme module re-read it. `window.__mioApplyTheme` is exposed for
    // exactly this in `theme.ts`.
    const fn = window.__mioApplyTheme
    if (typeof fn !== 'function') return 'no hook'
    fn(id)
    return 'ok'
  }, id)

// --- contrast, measured on the painted elements rather than on the tokens.
//
// A theme can pass every check above and still be unreadable: text at 42% alpha
// over a mid-grey surface is a token set that is internally consistent and
// illegible. WCAG's ratio is the only objective measure available here, so each
// region's real computed colours are compared. 4.5:1 is the normal-text threshold;
// the dim text the design uses for metadata is allowed 3:1.
const measureContrast = () =>
  page.evaluate(() => {
  const parse = (color) => {
    const m = color.match(/[\d.]+/g)
    if (!m) return null
    return { r: Number(m[0]), g: Number(m[1]), b: Number(m[2]), a: m[3] === undefined ? 1 : Number(m[3]) }
  }
  // Composite a translucent colour over its backdrop, which is what the eye sees.
  const over = (fg, bg) => ({
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1
  })
  const luminance = (c) => {
    const channel = (v) => {
      const s = v / 255
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
    }
    return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
  }
  const ratio = (a, b) => {
    const la = luminance(a)
    const lb = luminance(b)
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
  }
  /** The effective background behind an element: walk up until one is opaque. */
  const backdrop = (el) => {
    for (let node = el; node; node = node.parentElement) {
      const c = parse(getComputedStyle(node).backgroundColor)
      if (c && c.a > 0.5) return c
    }
    return { r: 0, g: 0, b: 0, a: 1 }
  }

  const out = []
  const targets = [
    ['.brand-name', 'brand'],
    ['.lib-head .lib-name', 'library header'],
    ['.facet .facet-name', 'facet'],
    ['.head-cell', 'column header'],
    ['.row .cell.name', 'file name'],
    ['.row .cell.dim', 'category cell'],
    ['.dock-name', 'dock file name'],
    ['.volume-value', 'volume readout'],
    ['.statusbar', 'status bar'],
    ['.table-foot', 'table footer']
  ]
  for (const [selector, label] of targets) {
    const el = document.querySelector(selector)
    if (!el) continue
    const fgRaw = parse(getComputedStyle(el).color)
    if (!fgRaw) continue
    const bg = backdrop(el)
    const fg = fgRaw.a < 1 ? over(fgRaw, bg) : fgRaw
    const r = ratio(fg, bg)
    // Metadata is deliberately dimmer; the design's own rule is 3:1 for it.
    const floor = /category|footer|status|volume|column/.test(label) ? 3 : 4.5
    out.push({ label, ratio: Number(r.toFixed(2)), floor, ok: r >= floor })
  }
  return out
})

// Walk every theme and check that the DOM actually changed.

const seen = new Map()
const contrastByTheme = new Map()
for (const id of THEMES) {
  const status = await apply(id)
  if (status !== 'ok') {
    problems.push(`no apply hook in the page: ${status}`)
    break
  }
  await page.waitForTimeout(250)
  const state = await probe()
  results.push({ id, ...state })

  if (state.applied !== id) problems.push(`${id}: dataset says ${state.applied}`)
  if (state.stored !== id) problems.push(`${id}: localStorage says ${state.stored}`)
  if (!state.accent) problems.push(`${id}: --theme-color-primary is empty`)
  if (state.bodyBg === 'rgba(0, 0, 0, 0)') problems.push(`${id}: body background is transparent`)

  const signature = `${state.bg}|${state.accent}|${state.dynamic}|${state.text}`
  if (seen.has(signature)) problems.push(`${id} paints the same tokens as ${seen.get(signature)}`)
  seen.set(signature, id)

  // A light theme must actually be light: the background has to be brighter than
  // the text, which is the one property that separates the two schemas. The schema
  // comes from the dataset, not the id — `tokyo-night` is dark without saying so.
  //
  // The token values can be either form: iX's own CSS writes `rgb()`, our override
  // block writes hex, and a hex string read as a digit sequence turns `#f3f3f3`
  // into 3/3/3. Parse both.
  const luminance = (color) => {
    const hex = color.trim().match(/^#([0-9a-f]{6})$/i)
    if (hex) {
      const n = Number.parseInt(hex[1], 16)
      const channel = (v) => {
        const s = v / 255
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
      }
      return (
        0.2126 * channel((n >> 16) & 255) +
        0.7152 * channel((n >> 8) & 255) +
        0.0722 * channel(n & 255)
      )
    }
    const m = color.match(/\d+(\.\d+)?/g)
    if (!m) return null
    const channel = (v) => {
      const s = v / 255
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
    }
    const [r, g, b] = m.map(Number)
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
  }
  const bgL = luminance(state.bg)
  const textL = luminance(state.text)
  if (bgL !== null && textL !== null) {
    if (state.schema === 'light' && bgL < textL) {
      problems.push(`${id}: background (${state.bg}) is darker than its text`)
    }
    if (state.schema === 'dark' && bgL > textL) {
      problems.push(`${id}: background (${state.bg}) is lighter than its text`)
    }
  }

  if (!state.overrideBlock) problems.push(`${id}: no override block was injected`)
  if (state.overrideLength < 1000) {
    problems.push(`${id}: the override block is only ${state.overrideLength} chars`)
  }

  // Contrast is measured per theme, on the painted elements. Checking it once,
  // after the loop, only ever tested whichever theme happened to be last.
  const contrast = await measureContrast()
  contrastByTheme.set(id, contrast)
  for (const c of contrast) {
    if (!c.ok) problems.push(`${id}: contrast ${c.label} is ${c.ratio}:1, needs ${c.floor}:1`)
  }

  // AUTO PLAY's on state is the accent filled with contrast text. The button's
  // token overrides are per-theme, so a light theme (white contrast text) and a
  // dark one (near-black) both have to clear 4.5:1. `contrastProbe` above measures
  // fixed selectors, so this is its own small probe against the live tokens.
  const autoPlay = await page.evaluate(() => {
    const root = getComputedStyle(document.documentElement)
    const parse = (color) => {
      const m = color.trim().match(/^#([0-9a-f]{6})$/i)
      if (m) {
        const n = Number.parseInt(m[1], 16)
        return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 }
      }
      const d = color.match(/[\d.]+/g)
      return d ? { r: Number(d[0]), g: Number(d[1]), b: Number(d[2]) } : null
    }
    const luminance = (c) => {
      const channel = (v) => {
        const s = v / 255
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
      }
      return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }
    const bg = parse(root.getPropertyValue('--theme-color-primary'))
    const fg = parse(root.getPropertyValue('--theme-color-primary--contrast'))
    if (!bg || !fg) return null
    const la = luminance(bg)
    const lb = luminance(fg)
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
  })
  if (autoPlay === null) {
    problems.push(`${id}: AUTO PLAY tokens are missing`)
  } else if (autoPlay < 4.5) {
    problems.push(`${id}: AUTO PLAY on-state text is ${autoPlay.toFixed(2)}:1, needs 4.5:1`)
  }
}

// The two Classic themes were removed. A profile that still has one stored has to
// land on the default instead of on a blank window, which is what `loadThemeId`
// promises: an unknown id falls back rather than being trusted.
await page.evaluate(() => localStorage.setItem('mio:theme', 'classic-dark'))
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(600)
const afterStale = await probe()
if (afterStale.applied !== DEFAULT_THEME) {
  problems.push(`a stored classic-dark applied ${afterStale.applied}, expected ${DEFAULT_THEME}`)
}
if (!afterStale.overrideBlock) problems.push('the fallback theme did not inject its tokens')

// Then confirm a non-default theme survives a reload: the theme is stored, and a
// reload is the only way to prove the head script reads it.
await apply('tokyo-night')
await page.waitForTimeout(200)
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(600)
const afterReload = await probe()
if (afterReload.applied !== 'tokyo-night') {
  problems.push(`after reload the theme is ${afterReload.applied}, expected tokyo-night`)
}
if (!afterReload.overrideBlock) problems.push('after reload the override block is missing')


// The contrast table, one row per theme and element, so a failing theme is named
// in the output rather than only in the problem list.
for (const [id, rows] of contrastByTheme) {
  const worst = rows.reduce((a, b) => (a.ratio / a.floor <= b.ratio / b.floor ? a : b), rows[0])
  console.log(`${id.padEnd(14)} worst ${worst.label} ${worst.ratio}:1 (floor ${worst.floor})`)
}

// And back to the default for the rest of the checks.
await apply(DEFAULT_THEME)

console.table(results.map((r) => ({
  theme: r.id,
  schema: r.schema,
  bg: r.bg,
  surface: r.surface,
  accent: r.accent,
  text: r.text,
  override: r.overrideBlock
})))
if (problems.length === 0) {
  console.log('\nthemes: OK')
} else {
  console.log('\ntheme problems:')
  for (const p of problems) console.log(`  - ${p}`)
}
await browser.close()
process.exit(problems.length === 0 ? 0 : 1)