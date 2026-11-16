// Screenshot, DOM dump and console-error capture for the running ReSlip window.
//
// Run from `slip-sound/` (it has playwright-core installed):
//   node <this file> [port] [out.png]
//
// Why CDP instead of a dev server: the point is to look at the built shell, with
// the real Rust process behind it. A dev-server page has no `invoke`, so every
// command fails and the screenshot proves nothing.
import { chromium } from 'playwright-core'

const port = process.argv[2] ?? '9223'
const outPng = process.argv[3] ?? 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch/reslip.png'

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const pages = browser.contexts().flatMap((c) => c.pages())
if (pages.length === 0) {
  console.error('no pages on the debugger; is the app running?')
  process.exit(1)
}
console.log('pages:', pages.map((p) => p.url()))

const page = pages[0]
const logs = []
page.on('console', (m) => logs.push(`${m.type()}: ${m.text()}`))
page.on('pageerror', (e) => logs.push(`pageerror: ${e.message}`))

await page.waitForTimeout(1500)
await page.screenshot({ path: outPng })
console.log('screenshot:', outPng)

console.log('--- innerText ---')
console.log((await page.evaluate(() => document.body.innerText)).slice(0, 2000))

console.log('--- console ---')
for (const line of logs.slice(0, 40)) console.log(line)

await browser.close()
