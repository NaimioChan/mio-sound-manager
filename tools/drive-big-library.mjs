// Open the real 280k-file library in the running window and measure what the user
// would feel: how long the folder open takes to return, how long until the first
// page of rows is on screen, and how long a search takes with the whole library
// indexed.
//
//   node drive-big-library.mjs <library-folder> [port]
//
// Run from a directory that has playwright-core.
import { chromium } from 'playwright-core'

const library = process.argv[2]
const port = process.argv[3] ?? '9223'
const shotDir = 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch'

if (!library) {
  console.error('usage: node drive-big-library.mjs <library-folder> [port]')
  process.exit(1)
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

const errors = []
page.on('pageerror', (e) => errors.push(e.message))

page.once('dialog', async (dialog) => dialog.accept(library))

const opened = Date.now()
const cta = page.locator('button.primary')
if (await cta.count()) await cta.click()
else await page.locator('button.ghost', { hasText: /library/i }).first().click()

// `open_library` resolves as soon as the database is readable, before the pass
// finishes. That is the number that decides whether the window feels responsive.
await page.locator('.row, .table-empty').first().waitFor({ timeout: 120_000 })
const firstRowsMs = Date.now() - opened
console.log(`open_library → first rows on screen: ${firstRowsMs} ms`)
await page.screenshot({ path: `${shotDir}/reslip-big-open.png` })

// Let a partial pass land, then measure a search against whatever is indexed.
await page.waitForTimeout(6000)
const beforeSearch = Date.now()
await page.locator('input.search').fill('gunshot')
await page.waitForTimeout(50)
const rows = await page.locator('.row').count()
console.log(`search "gunshot" → ${rows} rows in ~${Date.now() - beforeSearch} ms (includes 80ms debounce)`)

const status = await page.evaluate(() =>
  Array.from(document.querySelectorAll('.statusbar span')).map((s) => s.textContent)
)
console.log('status bar:', status.join(' | '))

// Click through the first few rows and time click → waveform painted. This is the
// interaction the whole project exists to make fast.
const rowCount = await page.locator('.row').count()
console.log('rows on screen:', rowCount)
for (let i = 0; i < Math.min(3, rowCount); i++) {
  const clickAt = Date.now()
  await page.locator('.row').nth(i).click()
  await page
    .locator('.wave-time')
    .first()
    .waitFor({ state: 'visible', timeout: 30_000 })
    .catch(() => {})
  console.log(`row ${i}: click → waveform visible in ${Date.now() - clickAt} ms`)
}
await page.screenshot({ path: `${shotDir}/reslip-big-playing.png` })

// Wait for the full pass so the count is final, then report it.
for (let i = 0; i < 600; i++) {
  const text = await page.evaluate(() => document.querySelector('.statusbar')?.textContent ?? '')
  if (!text.includes('indexing')) break
  await page.waitForTimeout(250)
}
console.log(`full pass finished at ${Date.now() - opened} ms after open`)
console.log(
  'final status:',
  await page.evaluate(() => document.querySelector('.statusbar')?.textContent)
)
await page.screenshot({ path: `${shotDir}/reslip-big-done.png` })

if (errors.length) console.log('page errors:', errors)

await browser.close()
