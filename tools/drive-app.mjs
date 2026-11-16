// Drive the running ReSlip window over CDP: open a library, click a row, wait for
// the waveform, screenshot each stage.
//
//   node drive-app.mjs <library-folder> [port]
//
// Run from a directory that has playwright-core (slip-sound/ has it).
import { chromium } from 'playwright-core'

const library = process.argv[2]
const port = process.argv[3] ?? '9223'
const shotDir = 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch'

if (!library) {
  console.error('usage: node drive-app.mjs <library-folder> [port]')
  process.exit(1)
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

const logs = []
page.on('console', (m) => logs.push(`${m.type()}: ${m.text()}`))
page.on('pageerror', (e) => logs.push(`pageerror: ${e.message}`))

// The library picker is a `window.prompt`. Auto-answering it is the only way to
// drive "open library" without a native dialog, and it exercises the same command
// the button calls.
page.once('dialog', async (dialog) => {
  console.log('dialog:', dialog.message())
  await dialog.accept(library)
})

// Two buttons open a library: the empty-state CTA and the top bar. Either one
// calls the same command, so pick whichever is on screen.
const emptyCta = page.locator('button.primary')
const topBar = page.locator('button.ghost', { hasText: /library/i }).first()
if (await emptyCta.count()) {
  await emptyCta.click()
} else {
  await topBar.click()
}
await page.waitForTimeout(800)

// Wait for the index pass to finish: the status bar shows "indexing x / y" while
// it runs and the sound count when it stops.
for (let i = 0; i < 120; i++) {
  const text = await page.evaluate(() => document.body.innerText)
  if (!text.includes('indexing')) break
  await page.waitForTimeout(250)
}
await page.screenshot({ path: `${shotDir}/reslip-library.png` })
console.log('--- after indexing ---')
console.log((await page.evaluate(() => document.body.innerText)).slice(0, 1200))

// Click the first data row and wait for the waveform payload.
const rows = page.locator('.row')
const count = await rows.count()
console.log('rows rendered:', count)
if (count > 0) {
  await rows.first().click()
  await page.waitForTimeout(1200)
  await page.screenshot({ path: `${shotDir}/reslip-playing.png` })
  console.log('--- after click ---')
  console.log((await page.evaluate(() => document.body.innerText)).slice(0, 1200))
}

// Type a query and check the table narrows.
const search = page.locator('input.search')
await search.fill('gunshot')
await page.waitForTimeout(500)
console.log('rows after search "gunshot":', await page.locator('.row').count())
await page.screenshot({ path: `${shotDir}/reslip-search.png` })

console.log('--- console ---')
for (const line of logs.slice(0, 40)) console.log(line)

await browser.close()
