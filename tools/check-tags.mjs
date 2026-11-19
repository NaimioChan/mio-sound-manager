// Checks for the P9 batch: the segmented toggle's left edge, the settings dialog
// reopening after the × closes it, the BPM column and sort, multi-select tags, the
// folder row's own menu, and the absence of subtitles.
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
await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.row', { timeout: 90_000 })
await page.waitForTimeout(1000)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

const rowCount = () => page.locator('.row').count()

/** The rail remembers its mode; start from Categories so the seam check is stable. */
await page.evaluate(() => localStorage.setItem('mio:sidebar-mode', 'categories'))
await page.evaluate(() => {
  const [cats] = document.querySelectorAll('.side-mode')
  if (!cats.classList.contains('on')) cats.click()
})
await page.waitForTimeout(500)

// ---------------------------------------------------------------- 1. toggle seam
// Both halves must paint a 1px border on all four sides while lit. The failure
// being checked for is a lit *second* half with no left border, which is what a
// shared seam with `border-left: none` produces.
const seam = await page.evaluate(() => {
  const [cats, folders] = document.querySelectorAll('.side-mode')
  const lit = (el) => el.classList.contains('on')
  const borders = (el) => {
    const s = getComputedStyle(el)
    return [s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth].join(' ')
  }
  return {
    catLit: lit(cats),
    folderLit: lit(folders),
    catBorders: borders(cats),
    folderBorders: borders(folders),
    catRight: Math.round(cats.getBoundingClientRect().right),
    folderLeft: Math.round(folders.getBoundingClientRect().left)
  }
})
check(seam.catLit && !seam.folderLit, 'Categories starts lit in categories mode', JSON.stringify(seam))

// Switch to Folders: the second half lights, and it must have a left border.
await page.locator('.side-mode').nth(1).click()
await page.waitForTimeout(300)
const folded = await page.evaluate(() => {
  const el = document.querySelectorAll('.side-mode')[1]
  const s = getComputedStyle(el)
  return {
    lit: el.classList.contains('on'),
    left: s.borderLeftWidth,
    right: s.borderRightWidth,
    color: s.borderTopColor
  }
})
check(folded.lit, 'clicking Folders lights the second half')
check(folded.left !== '0px', 'the lit second half keeps its left border', folded.left)
check(folded.right !== '0px', 'and its right border', folded.right)
// Light is the accent colour, so a missing edge is visible as a break in the box.
const accentBorder = await page.evaluate(() => {
  const el = document.querySelectorAll('.side-mode')[1]
  return getComputedStyle(el).borderTopColor
})
check(
  folded.color === accentBorder,
  'all four borders are the accent colour',
  folded.color
)

// The folder rail must be showing, and a folder row must exist for the next check.
// `.folder-node > .folder` is a real folder under the root. The bare `.folder`
// selector also matches the "All folders" row, which is the whole library and has
// no menu of its own.
await page.waitForSelector('.folder-node > .folder', { timeout: 20_000 })

// -------------------------------------------------- 2. folder row's own menu
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
await page.locator('.side-mode').nth(0).click()
await page.waitForTimeout(600)
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
check(
  afterDrag[1] === 'durationS' && afterDrag[0] === 'favorite',
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
// every column after it.
const movedCell = await page.evaluate(() => {
  const at = [...document.querySelectorAll('.head-cell')].findIndex(
    (el) => el.dataset.sortKey === 'durationS'
  )
  const cells = [...document.querySelectorAll('.row .cell')]
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

// Back to the shipped order so the rest of the run and the next script see default.
await page.evaluate(() => localStorage.removeItem('mio:column-order'))
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
