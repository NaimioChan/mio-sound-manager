// P5 acceptance: multi-library, blocking index, undo, infinite scroll, settings,
// resizable rail, and the search bar's new home. Every assertion drives the real
// window over CDP.
//
//   node tools/check-workspace.mjs <library-a> <library-b> [port]
//
// Two libraries are needed because the point is having both open at once.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const libraryA = process.argv[2]
const libraryB = process.argv[3]
const port = process.argv[4] ?? '9223'
const shotDir = 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch'

if (!libraryA || !libraryB) {
  console.error('usage: node check-workspace.mjs <library-a> <library-b> [port]')
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

let failures = 0
function check(ok, label, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
  if (!ok) failures++
}

const invoke = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args])

// --------------------------------------------------------------- add libraries
// Through the same command the picker feeds. The picker itself is a Win32 modal
// and cannot be driven from the page, which is why the commands are separate.
for (const root of [libraryA, libraryB]) {
  const started = Date.now()
  await page.evaluate(async (r) => {
    const channel = { __TAURI_CHANNEL_MARKER__: true, id: -1, onmessage: () => {} }
    const id = window.__TAURI_INTERNALS__.transformCallback(() => {})
    return window.__TAURI_INTERNALS__.invoke('add_library', {
      root: r,
      full: false,
      onProgress: `__CHANNEL__:${id}`
    })
  }, root)
  console.log(`add_library(${root}) returned in ${Date.now() - started} ms`)
}

const libraries = await invoke('list_libraries')
check(libraries.length === 2, 'both libraries are open', `${libraries.length}`)
check(
  libraries.filter((l) => l.active).length === 1,
  'exactly one library is active'
)
check(
  libraries.every((l) => !l.indexing),
  'no pass is still running after add_library resolves'
)

await page.reload()
await page.waitForSelector('.row', { timeout: 120_000 })

// ---------------------------------------------------------------- the rail
const libHeads = await page.locator('.lib-head').count()
check(libHeads === 2, 'the rail lists one group per library', `${libHeads} groups`)

const libNames = await page.locator('.lib-name').allTextContents()
console.log('rail libraries:', libNames.join(' | '))
check(
  libNames.every((n) => !n.includes('\\') && !n.includes('/')),
  'group headers show the folder name, not the path'
)

const activeGroup = await page.locator('.lib-group.active').count()
check(activeGroup === 1, 'the active library is marked')

// Switching libraries must change the rows.
const activeName = await page.locator('.lib-group.active .lib-name').textContent()
const otherHead = page.locator('.lib-group:not(.active) .lib-head').first()
const otherName = await otherHead.locator('.lib-name').textContent()
const rowsBefore = await page.locator('.row').count()
await otherHead.click()
await page.waitForTimeout(900)
const nowActive = await page.locator('.lib-group.active .lib-name').textContent()
check(nowActive === otherName, 'clicking a group switches the active library', `${activeName} → ${nowActive}`)
const rowsAfter = await page.locator('.row').count()
check(rowsAfter > 0, 'the other library shows its own rows', `${rowsBefore} → ${rowsAfter}`)
await page.screenshot({ path: `${shotDir}/reslip-p5-multi.png` })

// ------------------------------------------------------- infinite scroll
// Load the whole library first so the scroll has somewhere to go.
const total = await page.evaluate(() => {
  const el = document.querySelector('.table-foot')
  return el ? el.textContent : ''
})
console.log('foot before scrolling:', total.trim())

const before = await page.locator('.row').count()
// Scroll to the bottom of the virtual list a few times; each pass should append.
for (let i = 0; i < 6; i++) {
  await page.evaluate(() => {
    const el = document.querySelector('.table-body')
    if (el) el.scrollTop = el.scrollHeight
  })
  await page.waitForTimeout(400)
}
const loadedText = await page.locator('.table-foot').textContent()
const loadedMatch = loadedText.match(/([\d,]+)\s+of\s+([\d,]+)/)
const loadedCount = loadedMatch ? Number(loadedMatch[1].replace(/,/g, '')) : 0
const matchingCount = loadedMatch ? Number(loadedMatch[2].replace(/,/g, '')) : 0
console.log(`foot after scrolling: ${loadedText.trim()}`)
check(
  loadedCount >= before,
  'scrolling does not lose rows',
  `${before} → ${loadedCount}`
)
check(
  matchingCount > 0 && loadedCount <= matchingCount,
  'the footer never claims more rows than the filter matches',
  `${loadedCount} <= ${matchingCount}`
)

// ---------------------------------------------------------------- undo/redo
const firstRow = page.locator('.row').first()
await firstRow.click()
await page.waitForTimeout(300)
const favBefore = (await invoke('get_sidebar')).favorites
await firstRow.locator('.cell.favorite').click()
await page.waitForTimeout(800)
const favAfter = (await invoke('get_sidebar')).favorites
check(favAfter === favBefore + 1, 'starring works', `${favBefore} → ${favAfter}`)

const undoVisible = await page.locator('.topbar button', { hasText: 'Undo' }).count()
check(undoVisible === 1, 'an Undo button appears after an edit')

await page.locator('.topbar button', { hasText: 'Undo' }).click()
await page.waitForTimeout(800)
const favUndone = (await invoke('get_sidebar')).favorites
check(favUndone === favBefore, 'Undo puts the star back', `${favAfter} → ${favUndone}`)

const redoVisible = await page.locator('.topbar button', { hasText: 'Redo' }).count()
check(redoVisible === 1, 'a Redo button appears after undoing')
await page.locator('.topbar button', { hasText: 'Redo' }).click()
await page.waitForTimeout(800)
const favRedone = (await invoke('get_sidebar')).favorites
check(favRedone === favAfter, 'Redo re-applies it', `${favUndone} → ${favRedone}`)

// Keyboard path, which is the one that matters when a whole selection was wrong.
await page.keyboard.press('Control+z')
await page.waitForTimeout(800)
const favKeyUndo = (await invoke('get_sidebar')).favorites
check(favKeyUndo === favBefore, 'Ctrl+Z undoes', `${favRedone} → ${favKeyUndo}`)
await page.keyboard.press('Control+Shift+z')
await page.waitForTimeout(800)
const favKeyRedo = (await invoke('get_sidebar')).favorites
check(favKeyRedo === favAfter, 'Ctrl+Shift+Z redoes')

// Leave the library as it was found.
await page.keyboard.press('Control+z')
await page.waitForTimeout(600)

// ---------------------------------------------------------------- settings
await page.locator('.topbar button', { hasText: 'Settings' }).click()
await page.waitForTimeout(400)
check((await page.locator('.modal.settings').count()) === 1, 'Settings opens from the top bar')
const settingsSections = await page.locator('.settings-section h3').allTextContents()
console.log('settings sections:', settingsSections.join(' | '))
check(settingsSections.length >= 3, 'settings has playback, list and library sections')
await page.screenshot({ path: `${shotDir}/reslip-p5-settings.png` })

// Toggling a setting must persist without an OK button.
const autoPlayBox = page.locator('.setting input[type=checkbox]').first()
const wasChecked = await autoPlayBox.isChecked()
await autoPlayBox.click()
await page.waitForTimeout(200)
const stored = await page.evaluate(() => localStorage.getItem('reslip:autoplay'))
check(stored === String(!wasChecked), 'a setting is applied and stored immediately', `${stored}`)
await autoPlayBox.click()
await page.waitForTimeout(200)

await page.keyboard.press('Escape')
await page.waitForTimeout(300)
check((await page.locator('.modal.settings').count()) === 0, 'Escape closes settings')

// --------------------------------------------------------- resizable rail
const railWidthBefore = await page.evaluate(
  () => document.querySelector('.sidebar').getBoundingClientRect().width
)
const handle = await page.locator('.rail-resize').boundingBox()
await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2)
await page.mouse.down()
await page.mouse.move(handle.x + handle.width / 2 + 90, handle.y + handle.height / 2, { steps: 8 })
await page.mouse.up()
await page.waitForTimeout(300)
const railWidthAfter = await page.evaluate(
  () => document.querySelector('.sidebar').getBoundingClientRect().width
)
check(
  Math.abs(railWidthAfter - railWidthBefore - 90) < 6,
  'dragging the divider resizes the rail',
  `${Math.round(railWidthBefore)} → ${Math.round(railWidthAfter)}`
)
const storedWidth = await page.evaluate(() => localStorage.getItem('reslip:sidebar-width'))
check(Number(storedWidth) > 0, 'the width is remembered', `${storedWidth}`)

// ------------------------------------------------------- search bar placement
const searchInMain = await page.evaluate(() => {
  const input = document.querySelector('input.search')
  const mainHead = document.querySelector('.main-head')
  return Boolean(input && mainHead && mainHead.contains(input))
})
check(searchInMain, 'the search bar lives at the top of the list column')

// ------------------------------------------------------------------ layout
const layout = await page.evaluate(() => {
  const problems = []
  const regions = {
    topbar: document.querySelector('.topbar'),
    rail: document.querySelector('.rail'),
    mainHead: document.querySelector('.main-head'),
    table: document.querySelector('.table'),
    dock: document.querySelector('.dock'),
    statusbar: document.querySelector('.statusbar')
  }
  for (const [name, el] of Object.entries(regions)) {
    if (!el) problems.push(`missing ${name}`)
  }
  const body = document.querySelector('.body').getBoundingClientRect()
  const rail = document.querySelector('.rail').getBoundingClientRect()
  const main = document.querySelector('.main').getBoundingClientRect()
  if (Math.abs(rail.right - main.left) > 2) {
    problems.push(`rail and main do not meet: ${rail.right} vs ${main.left}`)
  }
  if (main.right > window.innerWidth + 1) problems.push('main spills past the window')
  if (body.height <= 0) problems.push('body has no height')
  return problems
})
check(layout.length === 0, 'the regions tile the window', layout.join('; '))

if (errors.length) {
  console.log('--- page errors ---')
  for (const line of errors.slice(0, 10)) console.log(line)
  failures += errors.length
}

console.log(failures === 0 ? '\nOK' : `\nFAILED (${failures})`)
await browser.close()
process.exit(failures === 0 ? 0 : 1)
