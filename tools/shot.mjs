// Drive the real Mio window over CDP: screenshot it and report what rendered.
//
//   node tools/shot.mjs [out.png]
//
// Uses the same playwright-core that the existing checks resolve out of the
// Slip Sound checkout, so this needs no extra install.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const out = process.argv[2] ?? 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch/mio.png'
const port = process.argv[3] ?? '9223'

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

await page.waitForSelector('.app', { timeout: 20_000 })
// The theme block and the iX element definitions both land asynchronously.
await page.waitForTimeout(1200)

const facts = await page.evaluate(() => {
  const region = (sel) => {
    const el = document.querySelector(sel)
    if (!el) return `${sel} absent`
    const r = el.getBoundingClientRect()
    return `${sel} ${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}×${Math.round(r.height)}`
  }
  const cs = getComputedStyle(document.documentElement)
  return {
    regions: ['.topbar', '.sidebar', '.table', '.dock', '.statusbar'].map(region),
    theme: document.documentElement.dataset.mioTheme ?? 'none',
    schema: document.documentElement.dataset.ixColorSchema ?? 'none',
    bg: cs.getPropertyValue('--theme-color-1').trim(),
    accent: cs.getPropertyValue('--theme-color-primary').trim(),
    ixTags: [...document.querySelectorAll('*')]
      .map((e) => e.tagName.toLowerCase())
      .filter((t) => t.startsWith('ix-'))
      .reduce((acc, t) => ((acc[t] = (acc[t] ?? 0) + 1), acc), {}),
    volume: (() => {
      const s = document.querySelector('#volume-slider')
      const r = s?.getBoundingClientRect()
      return s ? `#volume-slider ${Math.round(r.width)}×${Math.round(r.height)}` : 'missing'
    })(),
    dockHead: [...document.querySelectorAll('.dock-head > *')].map((el) => {
      const r = el.getBoundingClientRect()
      return `${el.tagName.toLowerCase()}.${(el.className || '').toString().split(' ')[0]} ${Math.round(r.left)}..${Math.round(r.right)}`
    }),
    rows: document.querySelectorAll('.row').length,
    libraries: [...document.querySelectorAll('.lib-name')].map((e) => e.textContent)
  }
})

console.log(JSON.stringify(facts, null, 1))
await page.screenshot({ path: out })
console.log(`shot: ${out}`)
await browser.close()
