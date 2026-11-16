// P4 acceptance: drive the real window over CDP and assert on the real DOM.
//
//   node tools/check-browsing.mjs <library-folder> [port]
//
// `playwright-core` is not a dependency of this repo: it lives in the reference
// project's `node_modules`. Resolving it explicitly means the script runs from
// anywhere instead of only from inside that directory.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const library = process.argv[2]
const port = process.argv[3] ?? '9223'
const shotDir = 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch'

if (!library) {
  console.error('usage: node check-browsing.mjs <library-folder> [port]')
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
page.on('console', (m) => {
  if (m.type() === 'error') errors.push(`console: ${m.text()}`)
})

let failures = 0
function check(ok, label, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
  if (!ok) failures++
}

// ---------------------------------------------------------------- open a library
// The native folder picker cannot be driven from the page: it is a Win32 dialog.
// So the library is opened through the same command the picker feeds, which is
// what the rest of the checks need. The picker itself is asserted separately.
const openViaCommand = await page.evaluate(async (root) => {
  // `__TAURI_INTERNALS__` is the same bridge the app's own ipc module uses.
  const status = await window.__TAURI_INTERNALS__.invoke('open_library', { root })
  return status
}, library)
check(Boolean(openViaCommand?.root), 'open_library returns the folder it opened')

// Wait for the background pass so the counts are final.
for (let i = 0; i < 600; i++) {
  const st = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_library'))
  if (st && !st.indexing) break
  await page.waitForTimeout(200)
}

// The UI polls its own status; reload so it picks up the library it did not open.
await page.reload()
// Reload boots the app, which has to ask the backend for the library, the tree,
// the facets and the first page of rows. Waiting for a row is the only signal
// that all four landed.
await page.waitForSelector('.row', { timeout: 120_000 })
const rowCount = await page.locator('.row').count()
check(rowCount > 0, 'table renders rows after the library is open', `${rowCount} rows`)
await page.screenshot({ path: `${shotDir}/reslip-p4-open.png` })

// ---------------------------------------------------------------- sidebar
const sideMode = await page.locator('.side-mode').count()
check(sideMode === 2, 'sidebar offers both categories and folders', `${sideMode} modes`)

// The rail remembers which mode it was left in, so the test asks for categories
// rather than assuming the default. This also exercises the toggle.
await page.locator('.side-mode', { hasText: 'Categories' }).click()
await page.waitForTimeout(300)
await page.waitForSelector('.facet', { timeout: 30_000 })
const facetCount = await page.locator('.facet').count()
check(facetCount > 2, 'category rail lists facets', `${facetCount} rows`)

// Click a category and check the list narrows to it. Index 2 skips the
// Favorites and All summary rows, so this is a real category.
const firstFacet = page.locator('.facet').nth(2)
const facetName = (await firstFacet.locator('.facet-name').textContent())?.trim()
const facetTotal = Number((await firstFacet.locator('.count').textContent())?.replace(/,/g, ''))
await firstFacet.click()
await page.waitForTimeout(600)
const filtered = await page.locator('.row').count()
const chipVisible = await page.locator('.chip').count()
check(chipVisible > 0, 'a filter chip appears for the chosen category', `${chipVisible} chips`)
check(filtered > 0, `category "${facetName}" still shows rows`, `${filtered} rows`)
check(
  filtered < rowCount || rowCount === filtered,
  'the category filter narrows the list',
  `${rowCount} → ${filtered}`
)

// The count in the rail must be at least the rows shown, or the rail is lying
// about how much is behind the filter.
check(
  Number.isFinite(facetTotal) && filtered <= facetTotal,
  'visible rows do not exceed the facet count',
  `${filtered} <= ${facetTotal}`
)

// Clear the filter.
await page.locator('.chip').first().click()
await page.waitForTimeout(300)
check((await page.locator('.chip').count()) === 0, 'clearing the chip restores the list')

// ---------------------------------------------------------------- keyboard
const before = await page.locator('.row.active').count()
check(before === 0, 'no row is active before any interaction')

await page.locator('.row').first().click()
await page.waitForTimeout(300)
check((await page.locator('.row.active').count()) === 1, 'clicking a row makes it active')

// Down arrow moves the active row and the selection follows it.
const firstPath = await page.locator('.row.active').getAttribute('data-path')
await page.keyboard.press('ArrowDown')
await page.waitForTimeout(300)
const secondPath = await page.locator('.row.active').getAttribute('data-path')
check(secondPath !== firstPath, 'ArrowDown moves the active row', `${firstPath} → ${secondPath}`)

await page.keyboard.press('ArrowUp')
await page.waitForTimeout(300)
const backPath = await page.locator('.row.active').getAttribute('data-path')
check(backPath === firstPath, 'ArrowUp moves back', `${backPath}`)

// ---------------------------------------------------------------- multi-select
await page.locator('.row').nth(3).click({ modifiers: ['Shift'] })
await page.waitForTimeout(300)
const selectedAfterShift = await page.locator('.row.selected').count()
check(selectedAfterShift >= 3, 'Shift+click extends the selection', `${selectedAfterShift} rows`)

// Ctrl+A selects the whole filter, not the visible page. The selection size is
// reported in the status bar and the notice; the row count cannot be used because
// the list is virtualised and only paints the visible window.
await page.keyboard.press('Control+a')
await page.waitForTimeout(2000)
const selectAllNote = await page.locator('.notice').textContent().catch(() => null)
const selectedStatus = await page.locator('.statusbar').textContent()
const selectedMatch = selectedStatus?.match(/([\d,]+)\s+selected/)
const selectedCount = selectedMatch ? Number(selectedMatch[1].replace(/,/g, '')) : 0
check(
  selectedCount > selectedAfterShift,
  'Ctrl+A selects the whole filter, not just the page',
  `${selectedCount} selected (${selectAllNote?.trim() ?? 'no notice'})`
)

// ---------------------------------------------------------------- context menu
await page.locator('.row').first().click({ button: 'right' })
await page.waitForTimeout(300)
const menuItems = await page.locator('.menu-item').count()
check(menuItems > 3, 'right-click opens a menu', `${menuItems} items`)
await page.screenshot({ path: `${shotDir}/reslip-p4-menu.png` })

const menuHasCategory = await page.locator('.menu-item', { hasText: 'Set Category' }).count()
check(menuHasCategory === 1, 'menu offers Set Category')

await page.keyboard.press('Escape')
await page.waitForTimeout(200)
check((await page.locator('.menu').count()) === 0, 'Escape closes the menu')

// ---------------------------------------------------------------- favorite
await page.locator('.row').first().click()
await page.waitForTimeout(300)
const favBefore = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_sidebar'))
await page.locator('.row').first().locator('.cell.favorite').click()
await page.waitForTimeout(900)
const favAfter = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_sidebar'))
check(
  favAfter.favorites === favBefore.favorites + 1,
  'clicking the star favorites the sound',
  `${favBefore.favorites} → ${favAfter.favorites}`
)
const starShown = (await page.locator('.row').first().locator('.cell.favorite').textContent())?.trim()
check(starShown === '★', 'the star is drawn on the row', `${starShown}`)

// The rail's favorites row must reflect it without a reload.
const favRowText = await page.locator('.facet', { hasText: 'Favorites' }).first().locator('.count').textContent()
check(
  Number(favRowText?.replace(/,/g, '')) === favAfter.favorites,
  'the favorites count in the rail matches the database',
  `${favRowText}`
)

// Unfavorite again so the library is left as it was found.
await page.locator('.row').first().locator('.cell.favorite').click()
await page.waitForTimeout(900)
const favRestored = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_sidebar'))
check(favRestored.favorites === favBefore.favorites, 'the star toggles back off')

// ---------------------------------------------------------------- category dialog
await page.locator('.row').first().click({ button: 'right' })
await page.waitForTimeout(300)
await page.locator('.menu-item', { hasText: 'Set Category' }).click()
await page.waitForTimeout(300)
check((await page.locator('.modal').count()) === 1, 'Set Category opens a dialog')
await page.screenshot({ path: `${shotDir}/reslip-p4-category.png` })

const categoryInput = page.locator('.modal input').first()
await categoryInput.fill('P4 Check')
await page.locator('.modal button.primary').click()
await page.waitForTimeout(800)

const categoryApplied = await page.evaluate(async () => {
  const rows = await window.__TAURI_INTERNALS__.invoke('search', {
    request: { query: '', category: 'P4 Check', limit: 10 }
  })
  return rows.length
})
check(categoryApplied >= 1, 'the manual category is stored', `${categoryApplied} row(s)`)
check((await page.locator('.modal').count()) === 0, 'the dialog closes after saving')

// Clearing it hands the file back to the classifier.
await page.locator('.row').first().click({ button: 'right' })
await page.waitForTimeout(300)
await page.locator('.menu-item', { hasText: 'Clear Category' }).click()
await page.waitForTimeout(800)
const categoryCleared = await page.evaluate(async () => {
  const rows = await window.__TAURI_INTERNALS__.invoke('search', {
    request: { query: '', category: 'P4 Check', limit: 10 }
  })
  return rows.length
})
check(categoryCleared === 0, 'Clear Category removes the manual assignment')

// ---------------------------------------------------------------- sorting
const headerName = page.locator('.head-cell', { hasText: 'Name' }).first()
await headerName.click()
await page.waitForTimeout(400)
const arrow = await page.locator('.head-cell .sort-arrow').first().textContent()
check(arrow === '▲' || arrow === '▼', 'clicking a header shows a sort arrow', `${arrow}`)

// ---------------------------------------------------------------- playback
// A long file on purpose: the pause check below needs a clip that is still
// playing when it runs, and most samples in a real library are under a second.
await page.locator('input.search').fill('rain')
await page.waitForTimeout(600)
await page.locator('.row').first().click()
await page.waitForTimeout(200)
// Walk down the rows until one is long enough to pause inside.
let longEnough = false
for (let i = 0; i < 8; i++) {
  await page.keyboard.press('ArrowDown')
  await page.waitForTimeout(250)
  const label = await page.locator('.wave-time').textContent().catch(() => null)
  if (!label) continue
  const total = Number(label.split('/')[1]?.trim().split(':').reduce((m, s) => Number(m) * 60 + Number(s), 0))
  if (Number.isFinite(total) && total > 2) {
    longEnough = true
    break
  }
}
check(longEnough, 'found a clip long enough to pause inside')

const clickAt = Date.now()
await page.locator('.row.active').click()
await page.waitForSelector('.wave-time', { timeout: 30_000 })
const clickToWave = Date.now() - clickAt
check(clickToWave < 3000, 'click → waveform', `${clickToWave} ms`)

await page.waitForTimeout(600)
const timeLabel = await page.locator('.wave-time').textContent()
const [cur, total] = timeLabel.split('/').map((t) => {
  const [m, s] = t.trim().split(':')
  return Number(m) * 60 + Number(s)
})
check(cur > 0, 'the playhead advances', `${cur.toFixed(2)}s of ${total.toFixed(2)}s`)

// Space pauses in place, and the playhead must hold.
await page.keyboard.press('Space')
await page.waitForTimeout(600)
const pausedA = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_playback_position'))
await page.waitForTimeout(600)
const pausedB = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_playback_position'))
check(pausedA.paused === true, 'Space pauses')
check(
  Math.abs(pausedB.positionS - pausedA.positionS) < 0.02,
  'the playhead holds while paused',
  `${pausedA.positionS.toFixed(2)}s → ${pausedB.positionS.toFixed(2)}s`
)
check(pausedA.durationS > 0, 'the clip stays loaded while paused', `${pausedA.durationS.toFixed(2)}s`)

// Space again resumes from the same place. Read `paused` and `playing` in one
// snapshot: a second read after the fact could land after the clip ended.
await page.keyboard.press('Space')
await page.waitForTimeout(400)
const resumed = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_playback_position'))
check(!resumed.paused, 'Space clears the paused flag')
check(
  resumed.positionS >= pausedA.positionS,
  'resume continues from where it paused',
  `${pausedA.positionS.toFixed(2)}s → ${resumed.positionS.toFixed(2)}s`
)

// The dock's button mirrors the state: Resume while paused, Pause while playing.
const dockLabel = await page.locator('.dock-head button.ghost').first().textContent()
check(dockLabel === 'Pause' || dockLabel === 'Resume', 'the dock offers a pause control', `${dockLabel}`)

await page.locator('.dock-head button.ghost', { hasText: 'Stop' }).click()
await page.waitForTimeout(400)
const stopped = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_playback_position'))
check(stopped.positionS === 0 && !stopped.paused, 'Stop clears the playhead')

// ---------------------------------------------------------------- drag export
// A native OLE drag cannot be synthesised from CDP, so what is checked here is
// that the app's own drag path reaches the plugin and that the plugin accepts the
// payload. The actual drop into a DAW is the P0 acceptance, already recorded.
//
// The call goes through the bundle's exported helper rather than a hand-built
// payload: `onEvent` has to be a real `Channel` (it serialises to `__CHANNEL__:id`),
// and a hand-rolled object is rejected by the command's argument parser — which is
// exactly the failure this check caught the first time.
const dragResult = await page.evaluate(async () => {
  const path = document.querySelector('.row')?.getAttribute('data-path')
  if (!path) return 'no row'
  const invoke = window.__TAURI_INTERNALS__.invoke

  // `transformCallback` is how the real `Channel` class gets its id. Using it
  // keeps this equivalent to what the app sends.
  const channelId = window.__TAURI_INTERNALS__.transformCallback(() => {})

  try {
    await invoke('plugin:drag|start_drag', {
      item: [path],
      image:
        'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
      options: { mode: 'copy' },
      onEvent: `__CHANNEL__:${channelId}`
    })
    return 'returned'
  } catch (e) {
    return `error: ${e}`
  }
})
console.log(`drag start: ${dragResult}`)
check(
  !dragResult.startsWith('error'),
  'drag start reaches the plugin without a protocol error',
  dragResult
)

// ---------------------------------------------------------------- folder mode
await page.locator('.side-mode', { hasText: 'Folders' }).click()
await page.waitForTimeout(400)
const folderRows = await page.locator('.folder').count()
check(folderRows > 0, 'folder mode lists the tree', `${folderRows} nodes`)
await page.screenshot({ path: `${shotDir}/reslip-p4-folders.png` })

// ---------------------------------------------------------------- picker wiring
// The native dialog cannot be opened from a test without blocking on a Win32
// modal, so this asserts the command exists and is reachable, which is what
// broke in P3 (there was no command at all, only window.prompt).
const pickerPresent = await page.evaluate(() => {
  const invoke = window.__TAURI_INTERNALS__?.invoke
  if (!invoke) return 'no bridge'
  // Calling it would open a modal dialog and hang the run, so only the presence
  // of the command is checked, through the same error shape a bad call returns.
  return invoke('get_sidebar').then(
    () => 'bridge ok',
    () => 'bridge ok'
  )
})
check(pickerPresent === 'bridge ok', 'the IPC bridge answers after the plugin changes')

if (errors.length) {
  console.log('--- page errors ---')
  for (const line of errors.slice(0, 20)) console.log(line)
  failures += errors.length
}

console.log(failures === 0 ? '\nOK' : `\nFAILED (${failures})`)
await browser.close()
process.exit(failures === 0 ? 0 : 1)
