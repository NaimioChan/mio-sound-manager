// Checks for the P9 batch: the rail's mode dropdown, the settings dialog
// reopening after the × closes it, the BPM column and sort, multi-select tags, the
// folder row's own menu, column widths/visibility/header menu, and the absence of
// subtitles.
//
//   node tools/check-tags.mjs [port]
//
// Drives the real window over CDP, like the other `check-*.mjs` scripts. Locale is
// pinned to `en` because the assertions read labels; copy coverage is
// `check-i18n.mjs`'s job.
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

// A previous run that failed mid-way can leave a hidden column or a dragged width
// behind, and every test after this one indexes `.row .cell` by the header order.
// Column state is per machine, not per library, so it has to be cleared here and
// restored at the end: a profile with a deliberately hidden column should survive a
// test run unchanged. The inspector is layout too, and a profile that left it open
// puts the table's right-hand drag targets under the panel — the drag then works,
// the pointer lands somewhere else, and the failure reads as "the resize broke".
const COLUMN_KEYS = [
  'mio:column-hidden',
  'mio:column-widths',
  'mio:column-order',
  'mio:inspector-open',
  'mio:inspector-width'
]
const columnSnapshot = await page.evaluate((keys) => {
  const saved = {}
  for (const key of keys) saved[key] = localStorage.getItem(key)
  return saved
}, COLUMN_KEYS)
await page.evaluate((keys) => {
  localStorage.setItem('mio:locale', 'en')
  for (const key of keys) localStorage.removeItem(key)
}, COLUMN_KEYS)
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(1000)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

const rowCount = () => page.locator('.row').count()

/**
 * The shipped column set for a profile that has never touched visibility. The
 * script cleared `mio:column-hidden` above, so this is what a new install gets:
 * no UCS columns and no Description, in the stated order.
 */
const defaultHeads = await page.evaluate(() =>
  [...document.querySelectorAll('.head-cell')].map((el) => el.dataset.sortKey)
)
check(
  defaultHeads.join(',') === 'favorite,cover,filename,category,durationS,channels,bpm,sampleRate,filesize',
  'the default columns are Cover … Size without UCS/Description',
  defaultHeads.join(',')
)

/**
 * Drive the rail's mode select the way the search field is driven: set the host's
 * property and dispatch the event iX would. Clicking an option inside the shadow
 * root needs the list to open and scroll first, which is flaky on CI machines.
 */
const pickMode = async (mode) => {
  await page.evaluate((mode) => {
    const host = document.querySelector('#sidebar-mode')
    if (!host) return
    host.value = mode
    host.dispatchEvent(new CustomEvent('valueChange', { detail: mode, bubbles: true }))
  }, mode)
  await page.waitForTimeout(800)
}

const modeState = () =>
  page.evaluate(() => ({
    value: document.querySelector('#sidebar-mode')?.value ?? null,
    attr: document.querySelector('.sidebar')?.dataset.sidebarMode ?? null,
    stored: localStorage.getItem('mio:sidebar-mode')
  }))

// ------------------------------------------------------ 1. the rail's mode select
// The rail remembers its mode; start from Categories so the next checks are stable.
await page.evaluate(() => localStorage.setItem('mio:sidebar-mode', 'categories'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

const initial = await modeState()
check(initial.value === 'categories', 'the rail opens in the stored mode', String(initial.value))
check(
  initial.attr === 'categories',
  'the sidebar reports its mode without reaching into the select shadow root',
  String(initial.attr)
)
check(
  (await page.locator('.side-mode').count()) === 0,
  'the segmented control is gone; the mode is one dropdown'
)
check(
  (await page.locator('.side-gear').count()) === 1,
  'the settings gear still sits beside the mode select'
)

// Folders: real folder rows, and the choice is remembered.
await pickMode('folders')
await page.waitForSelector('.folder-node > .folder', { timeout: 20_000 })
const foldersState = await modeState()
check(
  foldersState.attr === 'folders' && foldersState.stored === 'folders',
  'picking Folders switches the rail and stores the choice',
  `${foldersState.attr} / ${foldersState.stored}`
)

// Categories: category rows replace the folder tree.
await pickMode('categories')
await page.waitForSelector('.facet', { timeout: 20_000 })
const categoriesState = await modeState()
check(
  categoriesState.attr === 'categories' && categoriesState.stored === 'categories',
  'picking Categories switches back and stores it',
  String(categoriesState.stored)
)
check(
  (await page.locator('.folder-node').count()) === 0,
  'the folder tree is gone in categories mode'
)

// UCS: the tree is built from the native UCS catalog rather than the 22 classes.
await pickMode('ucs')
const ucsFacets = await page.locator('.facet[data-category]').count()
const ucsState = await modeState()
check(ucsState.attr === 'ucs', 'picking UCS selects the UCS tree', String(ucsState.attr))
if (ucsFacets > 0) {
  check(true, 'the UCS tree lists category rows', `${ucsFacets}`)
} else {
  console.log('note: the open library has no UCS rows; the UCS tree is empty for it')
}

// The old key migrated in: a profile from before the dropdown that had Categories
// selected must not fall back to UCS just because the new key is missing.
await page.evaluate(() => {
  localStorage.removeItem('mio:sidebar-mode')
  localStorage.setItem('mio:facet-mode', 'mio')
})
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
const migrated = await modeState()
check(
  migrated.attr === 'categories',
  'the old mio:facet-mode=mio profile migrates to Categories',
  String(migrated.attr)
)

// Selecting a mode must hand focus back to the window, or the table's arrow keys
// stay inside the select's filter input. The synthetic event cannot reproduce the
// focus iX sets on a real click, so this only guards the path after it.
await pickMode('ucs')
await pickMode('categories')
await page.waitForSelector('.facet', { timeout: 20_000 })
await page.keyboard.press('Home')
await page.keyboard.press('ArrowDown')
await page.waitForFunction(
  () => Number(document.querySelector('.row.active[data-path]')?.getAttribute('aria-posinset')) === 2,
  undefined,
  { timeout: 10_000 }
)
check(true, 'arrow keys move the list right after a mode change')

// The library header is a title, not a tree node: the twisty slot survives for
// indentation, but nothing in it can be clicked.
const libHeads = await page.evaluate(() => {
  const active = document.querySelector('.lib-group.active .lib-head')
  const defaultHead = document.querySelector('.lib-group:not(.active) .lib-head')
  const style = active ? getComputedStyle(active) : null
  return {
    clickableTwisties: document.querySelectorAll('.lib-head .twisty:not(.spacer)').length,
    spacer: document.querySelectorAll('.lib-head .twisty.spacer').length,
    shadow: style?.boxShadow ?? '',
    background: style?.backgroundColor ?? '',
    plainBackground: defaultHead ? getComputedStyle(defaultHead).backgroundColor : ''
  }
})
check(
  libHeads.clickableTwisties === 0,
  'the library header has no collapsible twisty',
  String(libHeads.clickableTwisties)
)
check(libHeads.spacer >= 1, 'the spacer keeps library names on the facet indent')
check(
  libHeads.shadow.includes('inset'),
  'the active library header carries its accent bar',
  libHeads.shadow
)
check(
  libHeads.background !== libHeads.plainBackground,
  'the active library header is a real selected state',
  `${libHeads.background} vs ${libHeads.plainBackground}`
)

// -------------------------------------------------- 2. folder row's own menu
// The rail is in Categories mode at this point; the folder tree only mounts under
// Folders.
await pickMode('folders')
await page.waitForSelector('.folder-node > .folder', { timeout: 20_000 })
const folderRows = page.locator('.folder-node > .folder')
const folderName = await folderRows.first().locator('.folder-name').textContent()
await folderRows.first().click({ button: 'right' })
await page.waitForTimeout(300)
const folderMenu = await page.evaluate(() =>
  [...document.querySelectorAll('.menu-item')].map((el) => el.dataset.menuId)
)
check(folderMenu.length >= 3, 'right-clicking a folder row opens a menu', `${folderMenu.length} items`)
check(
  folderMenu.includes('reveal') && folderMenu.includes('filter') && folderMenu.includes('copy-path'),
  'the folder menu carries reveal / filter / copy path',
  folderMenu.join(',')
)
// It has to be *our* panel, not the OS menu: ours is a `.menu` div with menu items.
check(
  (await page.locator('.menu[role="menu"]').count()) === 1,
  'the folder menu is the app-drawn panel'
)
await page.keyboard.press('Escape')
await page.waitForTimeout(250)
check((await page.locator('.menu').count()) === 0, 'Escape closes the folder menu')

// Filtering from the menu narrows the list to that folder.
const beforeFilter = await rowCount()
await folderRows.first().click({ button: 'right' })
await page.waitForTimeout(250)
await page.locator('.menu-item[data-menu-id="filter"]').click()
await page.waitForTimeout(700)
const afterFilter = await rowCount()
const folderChip = await page.locator('.filter-chip').allTextContents()
check(
  afterFilter > 0 && afterFilter <= beforeFilter,
  'filtering by folder narrows the list',
  `${beforeFilter} → ${afterFilter}`
)
check(
  folderChip.some((text) => (folderName ?? '').includes(text.trim())),
  'a folder chip appears',
  folderChip.join(' | ')
)
// Clear it so the tag checks start from the whole library.
await page.locator('.filter-chip').first().click()
await page.waitForTimeout(600)

// Back to Categories for the tag checks.
await pickMode('categories')
await page.waitForSelector('.facet', { timeout: 20_000 })

// ----------------------------------------------------------- 3. multi-select tags
// Two different category rows, Ctrl+clicked, must both stay lit and produce a
// combined count instead of replacing each other.
// `data-category` is on category and subcategory rows only; the Favorites and
// "All sounds" rows carry it empty or not at all. Selecting by attribute keeps the
// script off translated labels and off the two summary rows.
const categories = await page.evaluate(() =>
  [...document.querySelectorAll('.facet[data-category]')]
    .filter((el) => el.dataset.category && !el.classList.contains('sub'))
    .map((el) => el.dataset.category)
)
check(categories.length >= 3, 'the rail lists several categories', `${categories.length}`)

const facetBy = (name) =>
  page.locator(`.facet[data-category="${name}"]:not(.sub)`).first()
const litCategories = () =>
  page.evaluate(
    () =>
      [...document.querySelectorAll('.facet[data-category]:not(.sub).selected')].filter(
        (el) => el.dataset.category
      ).length
  )

// Pick two real categories.
const firstCat = categories[0]
const secondCat = categories[1]
await facetBy(firstCat).click()
await page.waitForTimeout(600)
check((await litCategories()) === 1, 'one click lights one row', firstCat)
const oneCount = await rowCount()

await facetBy(secondCat).click({ modifiers: ['Control'] })
await page.waitForTimeout(700)
const litCount = await litCategories()
const twoCount = await rowCount()
check(litCount === 2, 'Ctrl+click lights a second row', `${firstCat} + ${secondCat}`)
check(
  twoCount >= oneCount,
  'the second category adds rows rather than replacing the first',
  `${oneCount} → ${twoCount}`
)
const chips = await page.locator('.filter-chip').count()
check(chips >= 2, 'each lit category gets its own chip', `${chips} chips`)

// Ctrl+clicking a lit row removes just that one.
await facetBy(secondCat).click({ modifiers: ['Control'] })
await page.waitForTimeout(700)
check((await litCategories()) === 1, 'Ctrl+click removes one row and keeps the other')

// A plain click on the only lit row clears the filter.
await facetBy(firstCat).click()
await page.waitForTimeout(700)
check((await litCategories()) === 0, 'clicking the only lit row clears the filter')

// Subcategory rows are multi-select too. They mount only while their category is
// expanded, so the twisty has to be clicked first.
const expandable = page.locator('.facet[data-category]:not(.sub) .twisty').first()
if ((await expandable.count()) > 0) {
  await expandable.click()
  await page.waitForTimeout(400)
}
const sub = page.locator('.facet.sub').first()
if ((await sub.count()) > 0) {
  await sub.click()
  await page.waitForTimeout(700)
  check(
    (await page.locator('.facet.sub.selected').count()) >= 1,
    'a subcategory row lights'
  )
  const parentLit = await litCategories()
  check(parentLit >= 1, "a lit subcategory lights its parent category")
  await sub.click()
  await page.waitForTimeout(700)
  check(
    (await page.locator('.facet.sub.selected').count()) === 0,
    'clicking it again clears it'
  )
  check((await litCategories()) === 0, 'and its parent goes out with it')
} else {
  console.log('note: no subcategory rows expanded; skipped the subcategory checks')
}

// ------------------------------------------------------------------ 4. BPM column
const headKeys = await page.evaluate(() =>
  [...document.querySelectorAll('.head-cell')].map((el) => el.dataset.sortKey)
)
check(headKeys.includes('bpm'), 'the table has a BPM column', headKeys.join(','))

const bpmCells = await page.evaluate(() => {
  const head = [...document.querySelectorAll('.head-cell')].findIndex(
    (el) => el.dataset.sortKey === 'bpm'
  )
  const row = document.querySelector('.row')
  if (!row || head < 0) return null
  const cell = row.querySelectorAll('.cell')[head]
  return cell ? cell.textContent.trim() : null
})
check(bpmCells !== null, 'each row has a BPM cell', String(bpmCells))

// Sorting by it must not error and must keep the list populated.
await page.locator('.head-cell[data-sort-key="bpm"]').click()
await page.waitForTimeout(900)
const sortedCount = await rowCount()
check(sortedCount > 0, 'sorting by BPM keeps rows on screen', `${sortedCount}`)
const dir = await page.evaluate(() => {
  const head = document.querySelector('.head-cell[data-sort-key="bpm"]')
  return head?.querySelector('.sort-arrow')?.textContent ?? ''
})
check(dir === '▼', 'the first BPM click sorts descending', dir)
// Unmarked rows must be last: with a descending sort the tail is the em dashes.
const tail = await page.evaluate(() => {
  const rows = [...document.querySelectorAll('.row')]
  const head = [...document.querySelectorAll('.head-cell')].findIndex(
    (el) => el.dataset.sortKey === 'bpm'
  )
  const value = (row) => row.querySelectorAll('.cell')[head]?.textContent.trim() ?? ''
  return { first: value(rows[0]), last: value(rows[rows.length - 1]) }
})
check(tail.first !== '—', 'the top row has a real tempo', `${tail.first} … ${tail.last}`)

// ------------------------------------------------- 4b. columns can be reordered
//
// A header drag must move the column and the values under it together, persist the
// arrangement, and leave the sort click working. The star is pinned: it carries no
// label, so a user who drags it away cannot tell what the column is.
//
// Runs before the settings checks, which reload nothing but do click around.
const headerKeys = () =>
  page.evaluate(() => [...document.querySelectorAll('.head-cell')].map((el) => el.dataset.sortKey))
const dragHeader = async (fromKey, toKey, half) => {
  const src = await page.locator(`.head-cell[data-sort-key="${fromKey}"]`).boundingBox()
  const dst = await page.locator(`.head-cell[data-sort-key="${toKey}"]`).boundingBox()
  if (!src || !dst) return false
  await page.mouse.move(src.x + src.width / 2, src.y + src.height / 2)
  await page.mouse.down()
  const x = half === 'right' ? dst.x + dst.width - 5 : dst.x + 5
  await page.mouse.move(x, dst.y + dst.height / 2, { steps: 10 })
  await page.mouse.move(x, dst.y + dst.height / 2, { steps: 2 })
  await page.mouse.up()
  await page.waitForTimeout(350)
  return true
}

await page.evaluate(() => localStorage.removeItem('mio:column-order'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

const starDraggable = await page.evaluate(() =>
  document.querySelector('.head-cell[data-sort-key="favorite"]').getAttribute('draggable')
)
check(starDraggable === 'false', 'the star header is not draggable', String(starDraggable))

// Park the sort on a column the drag does not touch, so a stray click during the
// gesture would be visible as the arrow moving.
await page.locator('.head-cell[data-sort-key="channels"]').click()
await page.waitForTimeout(700)
const sortBeforeDrag = await page.evaluate(() => {
  const head = [...document.querySelectorAll('.head-cell')].find((el) => el.querySelector('.sort-arrow'))
  return `${head?.dataset.sortKey}/${head?.querySelector('.sort-arrow')?.textContent ?? ''}`
})

const beforeDrag = await headerKeys()
await dragHeader('durationS', 'filename', 'left')
const afterDrag = await headerKeys()
const durationAt = afterDrag.indexOf('durationS')
const filenameAt = afterDrag.indexOf('filename')
check(
  durationAt >= 0 && durationAt < filenameAt && afterDrag[0] === 'favorite',
  'dragging a header moves that column, and the star stays first',
  `${beforeDrag.join(',')} → ${afterDrag.join(',')}`
)

const sortAfterDrag = await page.evaluate(() => {
  const head = [...document.querySelectorAll('.head-cell')].find((el) => el.querySelector('.sort-arrow'))
  return `${head?.dataset.sortKey}/${head?.querySelector('.sort-arrow')?.textContent ?? ''}`
})
check(
  sortAfterDrag === sortBeforeDrag,
  'a drag does not re-sort the list on the way through',
  `${sortBeforeDrag} → ${sortAfterDrag}`
)

// The value under a moved header has to move with it, or the drag would relabel
// every column after it. The row's cells are read after the table settles: a
// reload or re-query can leave the row mounted with its cells not yet filled, and
// an empty read looks like a misalignment.
await page.waitForFunction(
  () => {
    const at = [...document.querySelectorAll('.head-cell')].findIndex(
      (el) => el.dataset.sortKey === 'durationS'
    )
    const cell = document.querySelectorAll('.row[data-path] .cell')[at]
    return /^\d+:\d\d/.test(cell?.textContent.trim() ?? '')
  },
  undefined,
  { timeout: 20_000 }
)
const movedCell = await page.evaluate(() => {
  const at = [...document.querySelectorAll('.head-cell')].findIndex(
    (el) => el.dataset.sortKey === 'durationS'
  )
  // `.row` alone also matches a skeleton row, which has no cells at all.
  const cells = [...document.querySelectorAll('.row[data-path] .cell')]
  return { at, text: cells[at]?.textContent.trim() ?? '', star: cells[0]?.textContent.trim() ?? '' }
})
check(
  /^\d+:\d\d/.test(movedCell.text) && movedCell.star === '☆',
  'the cells follow the header they belong to',
  `[${movedCell.at}] = ${movedCell.text}`
)

const storedOrder = await page.evaluate(() => localStorage.getItem('mio:column-order'))
check(
  typeof storedOrder === 'string' && storedOrder.includes('durationS'),
  'the arrangement is stored',
  String(storedOrder)
)

await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
const afterReload = await headerKeys()
check(
  afterReload.join(',') === afterDrag.join(','),
  'the arrangement survives a reload',
  afterReload.join(',')
)

// A reordered header must still sort, which is what a plain click does.
await page.locator('.head-cell[data-sort-key="bpm"]').click()
await page.waitForTimeout(700)
const sortableAfterDrag = await page.evaluate(
  () =>
    document.querySelector('.head-cell[data-sort-key="bpm"] .sort-arrow')?.textContent ?? 'none'
)
check(sortableAfterDrag === '▼', 'a moved header still sorts on click', sortableAfterDrag)

// --------------------------------------- 4b. AUTO PLAY survives its own toggle
// The on/off state used to be written with `class={…}`, which Solid assigns to
// `className` wholesale — that wiped Stencil's `hydrated` class, iX hides
// un-hydrated hosts, and the button disappeared on the first click and never came
// back. Assert on the marker plus a measured box, on and off, more than once.
const autoPlayLook = () =>
  page.evaluate(() => {
    const host = document.querySelector('#auto-play')
    const inner = host?.shadowRoot?.querySelector('button')
    const rect = host?.getBoundingClientRect()
    return {
      hydrated: !!host?.classList.contains('hydrated'),
      pressed: host?.pressed ?? null,
      width: Math.round(rect?.width ?? 0),
      visible: inner ? getComputedStyle(inner).visibility !== 'hidden' : false,
      background: inner ? getComputedStyle(inner).backgroundColor : null
    }
  })
const clickAutoPlay = async () => {
  await page.evaluate(() => {
    document.querySelector('#auto-play')?.shadowRoot?.querySelector('button')?.click()
  })
  await page.waitForTimeout(450)
}

const autoStart = await autoPlayLook()
const looks = [autoStart]
for (let i = 0; i < 3; i++) {
  await clickAutoPlay()
  looks.push(await autoPlayLook())
}
check(
  looks.every((look) => look.hydrated && look.visible && look.width > 20),
  'AUTO PLAY stays hydrated and visible through repeated toggles',
  JSON.stringify(looks.map((l) => `${l.pressed ? 'on' : 'off'}:${l.width}px`))
)
check(
  looks[0].pressed !== looks[1].pressed,
  'the toggle still changes state',
  `${looks[0].pressed} → ${looks[1].pressed}`
)
const onLook = looks.find((look) => look.pressed) ?? null
const offLook = looks.find((look) => !look.pressed) ?? null
check(
  onLook !== null && offLook !== null && onLook.background !== offLook.background,
  'the on and off states paint different backgrounds',
  `${onLook?.background} vs ${offLook?.background}`
)
// Put it back to the state the profile had, so a later check is not surprised.
if ((await autoPlayLook()).pressed !== autoStart.pressed) await clickAutoPlay()

// Back to the shipped order so the rest of the run and the next script see default.
await page.evaluate(() => localStorage.removeItem('mio:column-order'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

// ------------------------------------------------------- 4c. column widths
// Drag the BPM header's grip 60px to the right. The header and the rows must agree
// on every track afterwards: the three grids share one template, and a mismatch is
// values sitting under the wrong labels.
//
// The sort is parked on Name first, so a click that leaks out of the drag gesture
// shows up as the arrow appearing on BPM rather than as an ambiguous change.
const gridOf = (selector) =>
  page.evaluate((selector) => {
    const el = document.querySelector(selector)
    if (!el) return null
    return getComputedStyle(el).gridTemplateColumns.split(' ').map(Number.parseFloat)
  }, selector)
const sameTracks = (a, b, tolerance = 1) =>
  Array.isArray(a) && Array.isArray(b) && a.length === b.length &&
  a.every((width, i) => Math.abs(width - b[i]) <= tolerance)

await page.locator('.head-cell[data-sort-key="filename"]').click()
await page.waitForTimeout(700)

const grip = await page.locator('.head-cell[data-sort-key="bpm"] .head-resize').boundingBox()
check(grip !== null, 'a resizable column has a grip in its header', String(grip))
if (grip) {
  const headBefore = await gridOf('.table-head')
  const rowBefore = await gridOf('.row[data-path]')
  check(sameTracks(headBefore, rowBefore), 'header and rows start on the same tracks')
  const bpmBefore = headBefore?.[
    await page.evaluate(() =>
      [...document.querySelectorAll('.head-cell')].findIndex((el) => el.dataset.sortKey === 'bpm')
    )
  ]

  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2)
  await page.mouse.down()
  await page.mouse.move(grip.x + grip.width / 2 + 60, grip.y + grip.height / 2, { steps: 8 })
  await page.mouse.up()
  await page.waitForTimeout(500)

  const headAfter = await gridOf('.table-head')
  const rowAfter = await gridOf('.row[data-path]')
  const bpmAt = await page.evaluate(() =>
    [...document.querySelectorAll('.head-cell')].findIndex((el) => el.dataset.sortKey === 'bpm')
  )
  check(
    bpmAt >= 0 && headAfter && bpmBefore !== undefined && headAfter[bpmAt] > bpmBefore + 40,
    'dragging the grip widens the column',
    `${bpmBefore} → ${headAfter?.[bpmAt]}`
  )
  check(
    sameTracks(headAfter, rowAfter),
    'the rows follow the resized header track for track',
    JSON.stringify({ head: headAfter, row: rowAfter })
  )
  const storedWidths = await page.evaluate(() => localStorage.getItem('mio:column-widths'))
  check(
    typeof storedWidths === 'string' && storedWidths.includes('bpm'),
    'the dragged width is stored',
    String(storedWidths)
  )
  // A released grip leaves a click behind; it must have been swallowed, not used
  // as a sort.
  const leakedSort = await page.evaluate(
    () =>
      document.querySelector('.head-cell[data-sort-key="bpm"] .sort-arrow') !== null ||
      document.querySelector('.head-cell[data-sort-key="filename"] .sort-arrow') === null
  )
  check(!leakedSort, 'the click a finished resize leaves behind does not sort the column')

  await page.locator('.head-cell[data-sort-key="bpm"]').click()
  await page.waitForTimeout(700)
  const bpmSort = await page.evaluate(
    () => document.querySelector('.head-cell[data-sort-key="bpm"] .sort-arrow')?.textContent ?? 'none'
  )
  check(bpmSort === '▼', 'the column still sorts once the drag is over', bpmSort)

  await page.reload()
  await page.waitForSelector('.row', { timeout: 90_000 })
  await page.waitForTimeout(900)
  const reloadedBpm = await page.evaluate(() => {
    const head = document.querySelector('.table-head')
    const at = [...document.querySelectorAll('.head-cell')].findIndex(
      (el) => el.dataset.sortKey === 'bpm'
    )
    return at < 0
      ? null
      : Number.parseFloat(getComputedStyle(head).gridTemplateColumns.split(' ')[at])
  })
  check(
    reloadedBpm !== null && reloadedBpm > 100,
    'the width survives a reload',
    String(reloadedBpm)
  )
}

// ------------------------------------------------ 4d. hiding a column
// Through the settings dialog, which is the discoverable route. Unchecking BPM must
// remove its header and its cell, persist, and leave the sort on a visible column.
await page.evaluate(() => localStorage.removeItem('mio:column-widths'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

const openSettingsFromGear = async () => {
  await page.evaluate(() => {
    const host = document.querySelector('#settings-button')
    host?.shadowRoot?.querySelector('button')?.click()
  })
  await page.waitForTimeout(900)
}

await openSettingsFromGear()
const columnsSection = await page.evaluate(() => {
  const section = document.querySelector('#settings-columns')
  if (!section) return null
  const boxes = [...section.querySelectorAll('ix-checkbox')]
  return {
    visible: section.getBoundingClientRect().height > 0,
    boxes: boxes.length,
    // A trimmed iX bundle leaves the host un-upgraded: an empty inline element
    // that looks like a blank section. `hydrated` is Stencil's own marker, so it
    // is the one fact that separates "the control shipped" from "the tag is here".
    hydrated: boxes.filter((el) => el.classList.contains('hydrated')).length,
    boxHeights: boxes.map((el) => Math.round(el.getBoundingClientRect().height)),
    disabled: [...section.querySelectorAll('.column-check')]
      .filter((el) => el.querySelector('ix-checkbox')?.disabled)
      .map((el) => el.dataset.column ?? '')
  }
})
check(columnsSection !== null && columnsSection.visible, 'settings has a Columns section')
check(columnsSection?.boxes === 12, 'the section has one checkbox per column', String(columnsSection?.boxes))
check(
  columnsSection?.hydrated === 12,
  'every column checkbox is a real iX control, not an unupgraded tag',
  `${columnsSection?.hydrated}/12 hydrated`
)
check(
  columnsSection?.boxHeights.every((height) => height > 8),
  'the checkboxes have a rendered height',
  JSON.stringify(columnsSection?.boxHeights)
)
check((columnsSection?.disabled.length ?? 0) === 1, 'the pinned star cannot be hidden', JSON.stringify(columnsSection?.disabled))

// Toggle BPM off through the component's own event, the way iX would. `label` is
// set as a property by Solid, not an attribute, so the column is addressed by its
// own `data-column` instead.
await page.evaluate(() => {
  const section = document.querySelector('#settings-columns')
  const bpm = section?.querySelector('.column-check[data-column="bpm"] ix-checkbox')
  bpm?.dispatchEvent(new CustomEvent('checkedChange', { detail: false, bubbles: true }))
})
await page.waitForTimeout(500)
const hiddenNow = await page.evaluate(() => ({
  heads: document.querySelectorAll('.head-cell[data-sort-key="bpm"]').length,
  visibleHeads: document.querySelectorAll('.head-cell').length,
  cells: document.querySelector('.row[data-path]')?.querySelectorAll('.cell').length ?? 0,
  stored: localStorage.getItem('mio:column-hidden')
}))
check(hiddenNow.heads === 0, 'unchecking BPM removes its header')
check(
  hiddenNow.cells === hiddenNow.visibleHeads,
  'and its cell from every row',
  `${hiddenNow.cells} cells / ${hiddenNow.visibleHeads} headers`
)
check(
  typeof hiddenNow.stored === 'string' && hiddenNow.stored.includes('bpm'),
  'the hidden set is stored',
  String(hiddenNow.stored)
)

// Closing the dialog and reloading must keep the column hidden.
await page.keyboard.press('Escape')
await page.waitForTimeout(600)
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
check(
  (await page.locator('.head-cell[data-sort-key="bpm"]').count()) === 0,
  'a hidden column stays hidden after a reload'
)

// Hiding the sorted column has to move the sort to something visible, or the list
// is ordered by a key with no arrow and no way back.
await page.evaluate(() => localStorage.setItem('mio:column-hidden', '["bpm","sampleRate"]'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
const hiddenHeads = await page.evaluate(() => ({
  bpm: document.querySelectorAll('.head-cell[data-sort-key="bpm"]').length,
  rate: document.querySelectorAll('.head-cell[data-sort-key="sampleRate"]').length
}))
check(hiddenHeads.bpm === 0 && hiddenHeads.rate === 0, 'several columns can be hidden at once')

// ---------------------------------------------- 4e. header right-click menu
await page.evaluate(() => localStorage.removeItem('mio:column-hidden'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

await page.locator('.head-cell[data-sort-key="filename"]').click({ button: 'right' })
await page.waitForTimeout(300)
const headerMenu = await page.evaluate(() => ({
  ids: [...document.querySelectorAll('.menu-item')].map((el) => el.dataset.menuId),
  checked: [...document.querySelectorAll('.menu-item')].filter(
    (el) => el.querySelector('.menu-check')?.textContent?.trim() === '✓'
  ).length,
  disabled: [...document.querySelectorAll('.menu-item')].filter((el) => el.disabled).length
}))
check(
  headerMenu.ids.length === 13,
  'the header menu lists every column plus the settings entry',
  `${headerMenu.ids.length} items`
)
check(
  headerMenu.ids[headerMenu.ids.length - 1] === 'columns-settings',
  'the last entry opens column settings',
  headerMenu.ids.join(',')
)
check(
  headerMenu.checked === 9,
  'every visible column is ticked (UCS and Description start hidden)',
  String(headerMenu.checked)
)

// Clicking a column entry toggles it and closes the menu.
await page.locator('.menu-item[data-menu-id="column-bpm"]').click()
await page.waitForTimeout(600)
const afterMenuToggle = await page.evaluate(() => ({
  bpm: document.querySelectorAll('.head-cell[data-sort-key="bpm"]').length,
  menus: document.querySelectorAll('.menu').length
}))
check(afterMenuToggle.bpm === 0, 'the menu entry hides the column it names')
check(afterMenuToggle.menus === 0, 'and the menu closes on selection')

// "Column settings…" opens the dialog already scrolled to the section.
await page.evaluate(() => localStorage.removeItem('mio:column-hidden'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
await page.locator('.head-cell[data-sort-key="filename"]').click({ button: 'right' })
await page.waitForTimeout(300)
await page.locator('.menu-item[data-menu-id="columns-settings"]').click()
await page.waitForTimeout(1200)
const focusState = await page.evaluate(() => {
  const modal = [...document.querySelectorAll('ix-modal')].find((el) =>
    el.querySelector('.theme-card')
  )
  const dlg = modal?.shadowRoot?.querySelector('dialog')
  const section = document.querySelector('#settings-columns')
  if (!section) return { open: !!dlg?.open, inView: false, top: null }
  const rect = section.getBoundingClientRect()
  return {
    open: !!dlg?.open,
    inView: rect.top >= 0 && rect.bottom <= window.innerHeight + 2,
    top: Math.round(rect.top)
  }
})
check(focusState.open, 'the header menu opens the settings dialog')
check(focusState.inView, 'and it lands on the Columns section', `${focusState.top}px`)
await page.keyboard.press('Escape')
await page.waitForTimeout(600)

// ------------------------------------------------------ 4f. horizontal scroll
// Preset one very wide column, reload, and check the table really overflows and
// that the header follows the rows when scrolled — a header that stays put is the
// failure mode the translate is there to prevent.
await page.evaluate(() =>
  localStorage.setItem('mio:column-widths', JSON.stringify({ filename: 900 }))
)
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)
const overflow = await page.evaluate(() => {
  const body = document.querySelector('.table-body')
  // The filename track by *key*, not by position: a new column inserted before it
  // (the cover) would make a fixed index read someone else's width.
  const at = [...document.querySelectorAll('.head-cell')].findIndex(
    (el) => el.dataset.sortKey === 'filename'
  )
  return {
    scrollWidth: body?.scrollWidth ?? 0,
    clientWidth: body?.clientWidth ?? 0,
    tracks: Number.parseFloat(
      getComputedStyle(document.querySelector('.table-head')).gridTemplateColumns.split(' ')[at]
    )
  }
})
check(
  overflow.scrollWidth > overflow.clientWidth,
  'a wide table gets a horizontal scrollbar',
  `${overflow.scrollWidth} > ${overflow.clientWidth}`
)
check(overflow.tracks > 500, 'the preset width reached the grid', String(overflow.tracks))

await page.evaluate(() => {
  document.querySelector('.table-body').scrollLeft = 120
})
await page.waitForTimeout(300)
const aligned = await page.evaluate(() => {
  const head = document.querySelector('.head-cell[data-sort-key="filename"]')
  const row = document.querySelector('.row[data-path] .cell.name')
  const headAt = head?.getBoundingClientRect().left ?? 0
  const rowAt = row?.getBoundingClientRect().left ?? 0
  return {
    delta: Math.abs(headAt - rowAt),
    headAt: Math.round(headAt),
    rowAt: Math.round(rowAt)
  }
})
check(aligned.delta <= 1, 'the header follows the rows at a scrolled offset', JSON.stringify(aligned))

// Restore the defaults the rest of the run expects, and leave nothing behind for
// the next script.
await page.evaluate(
  ([keys, saved]) => {
    for (const key of keys) {
      if (saved[key] === null) localStorage.removeItem(key)
      else localStorage.setItem(key, saved[key])
    }
  },
  [COLUMN_KEYS, columnSnapshot]
)
// Settings and the settings blob are not touched by this script; only the three
// column keys are restored, exactly as the profile had them (or absent).
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(900)

// ------------------------------------------------------------------ 5. no subtitle
const subtitles = await page.evaluate(() => ({
  brand: document.querySelectorAll('.brand-sub').length,
  modalSub: [...document.querySelectorAll('ix-modal-header')].map(
    (el) => el.getAttribute('subheading') ?? el.subHeading ?? null
  ),
  text: document.querySelector('.brand')?.textContent?.trim() ?? ''
}))
check(subtitles.brand === 0, 'the top bar has no subtitle under the name', subtitles.text)

// The blocking index screen is checked by the same rule: the title and the live
// count are the whole card. A sentence under them explaining that the window is
// locked is a subtitle, and it was removed for that reason.
const indexNote = await page.evaluate(() => {
  const probe = document.createElement('div')
  probe.className = 'index-card'
  document.body.appendChild(probe)
  const notes = probe.querySelectorAll('.index-note').length
  // Also assert the class is gone from the stylesheet: a rule with no element is
  // how a removed line creeps back in as dead CSS.
  const declared = [...document.styleSheets]
    .flatMap((sheet) => {
      try {
        return [...sheet.cssRules]
      } catch {
        return []
      }
    })
    .some((rule) => rule.selectorText?.includes('.index-note'))
  probe.remove()
  return { notes, declared }
})
check(
  indexNote.notes === 0 && !indexNote.declared,
  'the index screen has no explanatory line under the count',
  JSON.stringify(indexNote)
)

// ------------------------------------------- 6. settings reopens after the ×
const openSettings = async () => {
  await page.evaluate(() => {
    const host = document.querySelector('#settings-button')
    host?.shadowRoot?.querySelector('button')?.click()
  })
  await page.waitForTimeout(900)
}
const settingsVisible = () =>
  page.evaluate(() => {
    const m = [...document.querySelectorAll('ix-modal')].find((el) =>
      el.querySelector('.theme-card')
    )
    if (!m) return false
    const dlg = m.shadowRoot?.querySelector('dialog')
    return m.classList.contains('visible') && !!dlg?.open
  })
const settingsGone = () =>
  page.evaluate(() =>
    [...document.querySelectorAll('ix-modal')].every(
      (el) => !el.classList.contains('visible')
    )
  )

await openSettings()
check(await settingsVisible(), 'settings opens the first time')
check(subtitles.modalSub.every((value) => value === null), 'the settings header has no subheading')
const slimControls = await page.evaluate(() => {
  const modal = [...document.querySelectorAll('ix-modal')].find((el) => el.querySelector('.theme-card'))
  const buttons = [...(modal?.querySelectorAll('ix-button.slim-button') ?? [])]
  const sizes = buttons.map((button) => Number.parseFloat(getComputedStyle(button).fontSize))
  const autoPlaySize = Number.parseFloat(getComputedStyle(document.querySelector('#auto-play')).fontSize)
  return {
    modalCount: buttons.length,
    modalSizes: sizes,
    autoPlaySize,
    autoPlayWeight: getComputedStyle(document.querySelector('#auto-play')).fontWeight
  }
})
check(slimControls.modalCount >= 2, 'Reveal and Open library use the slim button style', JSON.stringify(slimControls.modalSizes))
check(
  slimControls.modalSizes.every((size) => size < 12),
  'settings action text uses the smaller font size',
  JSON.stringify(slimControls.modalSizes)
)
check(
  slimControls.autoPlaySize < 12 && slimControls.autoPlayWeight === '400',
  'AUTO PLAY uses non-bold slim text',
  `${slimControls.autoPlaySize}px / ${slimControls.autoPlayWeight}`
)

// ------------------------------------------------- 7. no explanatory small print
// The rule is "no subtitles", and a settings row's helper line is one: a
// `.setting-hint` is allowed only where it carries state the label cannot — the
// device error, the format the engine actually opened, a library's count and
// path. A hint holding a sentence of explanation is the bug being checked for, so
// the test is on shape rather than on wording: no full stop, no "how"/"when",
// and short.
const hints = await page.evaluate(() => {
  const modal = [...document.querySelectorAll('ix-modal')].find((el) =>
    el.querySelector('.theme-card')
  )
  if (!modal) return null
  return {
    hints: [...modal.querySelectorAll('.setting-hint')].map((el) => el.textContent.trim()),
    themeCardHints: modal.querySelectorAll('.theme-card-hint').length,
    themeCardNames: [...modal.querySelectorAll('.theme-card-name')].map((el) =>
      el.textContent.trim()
    ),
    themeCardTitles: [...modal.querySelectorAll('.theme-card')].map((el) => el.title)
  }
})
check(hints !== null, 'the settings modal is in the DOM for the copy check')

/** A hint that reports state, not one that explains a setting. */
const reportsState = (text) =>
  /(Hz|\d+ ch|ms$)/.test(text) || // the engine's format and period
  /\\/.test(text) || // a library path
  /\d[\d,]*\s*\w/.test(text) || // a count
  /\b(error|failed|no |not )/i.test(text) // why something is missing
const offenders = (hints?.hints ?? []).filter((text) => !reportsState(text))
check(
  offenders.length === 0,
  'every remaining settings hint reports state rather than explaining the row',
  offenders.join(' | ')
)
check(hints?.themeCardHints === 0, 'the theme cards carry no description line')
check(
  (hints?.themeCardNames.length ?? 0) >= 4,
  'the theme cards still show their names',
  hints?.themeCardNames.join(',')
)
check(
  (hints?.themeCardTitles ?? []).every((title) => !/ for |as shipped|inverted/i.test(title)),
  'the theme card tooltip is the name, not the old description',
  hints?.themeCardTitles.join(' | ')
)

// ------------------------------------------------------- 8. interface scale
const zoomRow = await page.evaluate(() => {
  const modal = [...document.querySelectorAll('ix-modal')].find((el) =>
    el.querySelector('.theme-card')
  )
  const row = [...(modal?.querySelectorAll('.setting') ?? [])].find(
    (el) => el.querySelector('.setting-label')?.textContent?.trim() === 'Interface scale'
  )
  const select = row?.querySelector('ix-select')
  return {
    found: !!row,
    options: [...(select?.querySelectorAll('ix-select-item') ?? [])].map((el) => el.value)
  }
})
check(zoomRow.found, 'settings has an interface-scale row')
check(
  zoomRow.options.includes('100') && zoomRow.options.length >= 4,
  'the scale row offers several steps including 100%',
  zoomRow.options.join(',')
)

// The scale is a webview property, so the assertion is on the window, not on the
// select: at 150% the same physical window holds fewer CSS pixels, which is what
// `innerWidth` reports.
const innerWidthNow = () => page.evaluate(() => window.innerWidth)
const widthAt100 = await (async () => {
  await page.evaluate(() => window.__mioApplyZoom?.(100))
  await page.waitForTimeout(400)
  return innerWidthNow()
})()
const widthAt150 = await (async () => {
  await page.evaluate(() => window.__mioApplyZoom?.(150))
  await page.waitForTimeout(400)
  return innerWidthNow()
})()
check(
  widthAt150 < widthAt100 * 0.8,
  'the interface really scales: 150% leaves fewer CSS pixels in the window',
  `${widthAt100} → ${widthAt150}`
)
// Put it back so the rest of the script measures the default layout.
await page.evaluate(() => window.__mioApplyZoom?.(100))
await page.waitForTimeout(400)

// The × lives in the modal header's shadow root.
await page.evaluate(() => {
  const header = [...document.querySelectorAll('ix-modal-header')].find((el) =>
    el.closest('ix-modal')?.querySelector('.theme-card')
  )
  header?.shadowRoot?.querySelector('.modal-close')?.click()
})
await page.waitForTimeout(1200)
check(await settingsGone(), 'the × closes the settings dialog')

await openSettings()
check(await settingsVisible(), 'clicking the gear again reopens settings')
// And a third round, because the bug left the state stuck rather than off by one.
await page.evaluate(() => {
  const header = [...document.querySelectorAll('ix-modal-header')].find((el) =>
    el.closest('ix-modal')?.querySelector('.theme-card')
  )
  header?.shadowRoot?.querySelector('.modal-close')?.click()
})
await page.waitForTimeout(1200)
await openSettings()
check(await settingsVisible(), 'and a third time')
// Escape has to leave the same clean state.
await page.keyboard.press('Escape')
await page.waitForTimeout(900)
check(await settingsGone(), 'Escape closes it')
await openSettings()
check(await settingsVisible(), 'and the gear still works after Escape')

if (problems.length === 0) {
  console.log('\ntags/menus: OK')
} else {
  console.log('\nproblems:')
  for (const p of problems) console.log(`  - ${p}`)
}
await browser.close()
process.exit(problems.length === 0 ? 0 : 1)
