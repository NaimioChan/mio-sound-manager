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
await page.waitForSelector('.app', { timeout: 20_000 })

const problems = []
const results = []

const THEMES = [
  'classic-dark',
  'classic-light',
  'studio-dark',
  'studio-light',
  'amber-console',
  'frost-dark'
]

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
      // The override block is injected, not shipped, so its presence is the
      // difference between "iX's own palette" and "a themed one".
      overrideBlock: !!document.getElementById('mio-theme'),
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

// Walk every theme and check that the DOM actually changed.
const seen = new Map()
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
  // the text, which is the one property that separates the two schemas.
  const luminance = (color) => {
    const m = color.match(/\d+(\.\d+)?/g)
    if (!m) return null
    const [r, g, b] = m.map(Number)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b
  }
  const bgL = luminance(state.bg)
  const textL = luminance(state.text)
  if (bgL !== null && textL !== null) {
    if (id.includes('light') && bgL < textL) {
      problems.push(`${id}: background (${state.bg}) is darker than its text`)
    }
    if (id.includes('dark') && bgL > textL) {
      problems.push(`${id}: background (${state.bg}) is lighter than its text`)
    }
  }

  // Classic themes must not leave an override block behind: that is how "iX as
  // shipped" stays exactly that.
  if (id.startsWith('classic') && state.overrideBlock) {
    problems.push(`${id}: an override block is still injected`)
  }
  if (!id.startsWith('classic') && !state.overrideBlock) {
    problems.push(`${id}: no override block was injected`)
  }
}

// Back to the default, then confirm it survives a reload: the theme is stored, and
// a reload is the only way to prove the head script reads it.
await apply('studio-dark')
await page.waitForTimeout(200)
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(600)
const afterReload = await probe()
if (afterReload.applied !== 'studio-dark') {
  problems.push(`after reload the theme is ${afterReload.applied}, expected studio-dark`)
}
if (!afterReload.overrideBlock) problems.push('after reload the override block is missing')

// --- contrast, measured on the painted elements rather than on the tokens.
//
// A theme can pass every check above and still be unreadable: text at 42% alpha
// over a mid-grey surface is a token set that is internally consistent and
// illegible. WCAG's ratio is the only objective measure available here, so each
// region's real computed colours are compared. 4.5:1 is the normal-text threshold;
// the dim text the design uses for metadata is allowed 3:1.
const contrastProblems = await page.evaluate(() => {
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

for (const c of contrastProblems) {
  if (!c.ok) problems.push(`contrast: ${c.label} is ${c.ratio}:1, needs ${c.floor}:1`)
}
console.table(contrastProblems)

// And back to the default for the rest of the checks.
await apply('classic-dark')

console.table(results.map((r) => ({
  theme: r.id,
  schema: r.schema,
  bg: r.bg,
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
