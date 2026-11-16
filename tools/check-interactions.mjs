// Interaction checks against the running window: stop halts the playhead, clicking
// the waveform seeks, and the playhead keeps moving afterwards.
//
//   node check-interactions.mjs [port]
//
// `playwright-core` is not a dependency of this repo: it is resolved out of the
// reference project's `node_modules`, which lets the script run from anywhere.
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

const errors = []
page.on('pageerror', (e) => errors.push(e.message))

/** Read the playhead time and the clip duration from the dock's time label. */
async function readTimes() {
  const label = await page.locator('.wave-time').textContent().catch(() => null)
  if (!label) return null
  const [cur, total] = label.split('/')
  const parse = (text) => {
    const [m, s] = text.trim().split(':')
    return Number(m) * 60 + Number(s)
  }
  return { position: parse(cur), duration: parse(total) }
}

const rows = page.locator('.row')
if ((await rows.count()) === 0) {
  console.error('no rows: open a library first')
  process.exit(1)
}

// Search something that returns long files: a short sample finishes before the
// checks below can run, and a finished clip is a legitimately parked playhead.
await page.locator('input.search').fill('rain')
// Wait for the query to land instead of sleeping: while a search is in flight the
// table still shows the old rows, and a click on one of them is applied to a node
// that is about to be replaced — which silently drops the play request.
await page
  .waitForSelector('.table:not([data-querying="true"])', { timeout: 5000 })
  .catch(() => {})
await page.waitForTimeout(150)
const candidates = await rows.count()
console.log('rain rows:', candidates)
await rows.first().click()
await page.waitForTimeout(600)

const first = await readTimes()
await page.waitForTimeout(500)
const later = await readTimes()
if (!first || !later) {
  console.error('FAIL: no waveform panel')
  process.exit(1)
}
console.log(`duration: ${first.duration.toFixed(2)}s`)
console.log(`playhead: ${first.position.toFixed(2)}s → ${later.position.toFixed(2)}s`)
if (later.position <= first.position) {
  console.error('FAIL: playhead did not advance')
  process.exitCode = 1
}

// Click at 75% of the waveform; playback should resume from there.
const box = await page.locator('canvas.wave').boundingBox()
await page.mouse.click(box.x + box.width * 0.75, box.y + box.height / 2)
await page.waitForTimeout(700)
const seeked = await readTimes()
if (!seeked) {
  console.error('FAIL: no waveform panel after seek')
  process.exit(1)
}
const target = seeked.duration * 0.75
console.log(`after seek to 75%: ${seeked.position.toFixed(2)}s (expected ≥ ${target.toFixed(2)})`)
if (seeked.position < target - 0.5) {
  console.error('FAIL: seek did not move the playhead')
  process.exitCode = 1
}

// Stop must park the playhead and leave it parked. The button is icon-only, so
// it is found by its accessible name rather than by its text.
await page.locator('button.ghost[aria-label="Stop"]').click()
await page.waitForTimeout(500)
const stoppedA = await readTimes()
await page.waitForTimeout(600)
const stoppedB = await readTimes()
if (!stoppedA || !stoppedB) {
  console.error('FAIL: no waveform panel after stop')
  process.exit(1)
}
console.log(`stopped: ${stoppedA.position.toFixed(2)}s then ${stoppedB.position.toFixed(2)}s`)
if (Math.abs(stoppedB.position - stoppedA.position) > 0.05) {
  console.error('FAIL: playhead kept moving after stop')
  process.exitCode = 1
}

if (errors.length) {
  console.error('page errors:', errors)
  process.exitCode = 1
}

console.log(process.exitCode ? 'FAILED' : 'OK')
await browser.close()
