// Smoke-test a shipped build: does the window come up with the new features?
//
//   node tools/check-shipped.mjs [port]
//
// Run against a build launched from the *packaged* artifact (the zip's exe), not
// `target/release`, so this asserts on what a user actually gets. Every check is a
// fact that only exists if the right code shipped: the version string, ASIO being
// compiled in, and the three new dock controls.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9227'
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

const invoke = (cmd) =>
  page.evaluate((cmd) => window.__TAURI_INTERNALS__.invoke(cmd), cmd)

await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.app', { timeout: 30_000 })
await page.waitForTimeout(1500)

const title = await page.title()
check(title === 'Mio Sound Manager', 'the window comes up', title)

// ASIO compiled in: this is the flag that tells a product build from a dev one.
check((await invoke('asio_compiled')) === true, 'the shipped build has ASIO')

const status = await invoke('get_status')
check(status.audioAvailable === true, 'the engine opened a device', status.audioError ?? '')

// The three new controls. `#transport-loop` and `#volume-mute` only exist after
// this release; the legend is the mouse guide under the waveform.
const loopButton = await page.locator('#transport-loop').count()
check(loopButton === 1, 'the loop control shipped', String(loopButton))

const muteButton = await page.locator('#volume-mute').count()
check(muteButton === 1, 'the mute control shipped', String(muteButton))

const hints = await page.evaluate(
  () => document.querySelectorAll('.dock-hints span').length
)
check(hints >= 5, 'the mouse legend shipped', `${hints} entries`)

// The loop command exists and round-trips.
const loop = await invoke('get_loop')
check(
  typeof loop.enabled === 'boolean',
  'get_loop answers',
  JSON.stringify(loop)
)

// Version, read from the binary's own config rather than the tag.
const version = await page.evaluate(() => {
  const meta = document.querySelector('meta[name="version"]')
  return meta?.content ?? null
})
if (version) check(version.startsWith('0.1.1'), 'the version is 0.1.1', version)

await browser.close()
if (problems.length === 0) {
  console.log('\nshipped: OK')
  process.exit(0)
}
console.log('\nshipped problems:')
for (const p of problems) console.log(`  - ${p}`)
process.exit(1)
