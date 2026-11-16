// The blocking index screen: it must appear while a pass runs, cover the window,
// and actually swallow input. Checked by starting a real reindex through the UI
// and asserting the overlay's presence and behaviour while it is up.
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

let failures = 0
function check(ok, label, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
  if (!ok) failures++
}

// Make sure a library is open and the window is idle.
const libraries = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_libraries'))
if (libraries.length === 0) {
  console.error('no library open; open one first')
  process.exit(1)
}

// Trigger the reindex the way a user does: Settings → Reindex. The top-bar
// button is icon-only, so it is located by its accessible name.
await page.locator('.topbar button[aria-label="Settings"]').click()
await page.waitForTimeout(300)
const reindex = page.locator('.settings-actions button', { hasText: 'Reindex' }).first()
await reindex.click()

// The overlay must be up almost immediately — it is set before the command is
// awaited, so there is no window in which the app looks idle while indexing.
const appeared = await page
  .waitForSelector('.index-overlay', { timeout: 5000 })
  .then(() => true)
  .catch(() => false)
check(appeared, 'the overlay appears as soon as the pass starts')

if (appeared) {
  const covering = await page.evaluate(() => {
    const el = document.querySelector('.index-overlay')
    const r = el.getBoundingClientRect()
    return {
      full: r.width >= window.innerWidth - 1 && r.height >= window.innerHeight - 1,
      cursor: getComputedStyle(el).cursor,
      zIndex: getComputedStyle(el).zIndex
    }
  })
  check(covering.full, 'the overlay covers the whole window')
  check(covering.cursor === 'progress', 'the cursor says something is happening', covering.cursor)

  // Input must not reach the table. A click on a row under the overlay must not
  // change the selection.
  const before = await page.locator('.row.active').count()
  const row = await page.locator('.row').first().boundingBox().catch(() => null)
  if (row) {
    await page.mouse.click(row.x + 40, row.y + row.height / 2)
    await page.waitForTimeout(200)
    const after = await page.locator('.row.active').count()
    check(
      before === after,
      'clicks do not reach the list while indexing',
      `${before} → ${after} active`
    )
  }

  // Keyboard must not reach the table either: Space must not start playback.
  const posBefore = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke('get_playback_position')
  )
  await page.keyboard.press('Space')
  await page.waitForTimeout(300)
  const posAfter = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke('get_playback_position')
  )
  check(
    posBefore.positionS === posAfter.positionS && posBefore.playing === posAfter.playing,
    'Space does not start playback while indexing'
  )

  await page.screenshot({ path: 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch/reslip-p5-indexing.png' })

  // Wait for the pass to finish, then confirm the overlay is gone.
  const finished = await page
    .waitForSelector('.index-overlay', { state: 'detached', timeout: 300_000 })
    .then(() => true)
    .catch(() => false)
  check(finished, 'the overlay goes away when the pass finishes')

  const rows = await page.locator('.row').count()
  check(rows > 0, 'the list is usable afterwards', `${rows} rows`)
}

console.log(failures === 0 ? '\nOK' : `\nFAILED (${failures})`)
await browser.close()
process.exit(failures === 0 ? 0 : 1)
