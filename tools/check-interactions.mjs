// Interaction checks for the rewritten UI.
//
//   node tools/check-interactions.mjs [port]
//
// Covers what the rewrite touched: the iX search field, sort headers, selection
// (click / Ctrl / Shift), the row menu, the AUTO PLAY toggle, the dock transport
// and seeking on the waveform.
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
// A previous run can leave a modal open, which blocks every click. Reload first:
// the checks are all stateless from the app's point of view, and a clean page is
// the only reliable starting state for a script that drives a real window.
// These checks assert on English copy, so the locale is pinned before the reload.
// Copy coverage is `check-i18n.mjs`'s job, not this script's.
await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.row[data-path]', { timeout: 60_000 })
await page.waitForTimeout(1000)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

// --- search through the iX input, which emits `valueChange` from inside a shadow
//     root; typing into the host does not work, so the event is dispatched the way
//     the component does it.
const search = async (text) => {
  await page.evaluate((text) => {
    const input = document.querySelector('#search-input')
    input.value = text
    input.dispatchEvent(new CustomEvent('valueChange', { detail: text, bubbles: true }))
  }, text)
  await page.waitForFunction(
    () => document.querySelector('.table')?.dataset.querying === 'false',
    undefined,
    { timeout: 20_000 }
  )
  await page.waitForFunction(() => document.querySelector('.table')?.dataset.epoch === document.querySelector('.table')?.dataset.segmentsEpoch, undefined, { timeout: 20_000 })
  await page.waitForTimeout(80)
}

const rowCount = () => page.locator('.row[data-path]').count()
const total = await rowCount()
check(total > 0, 'the list has rows', `${total}`)

const first = await page.locator('.row[data-path]').first().getAttribute('data-path')
check(!!first, 'rows carry their path', first?.slice(-40))

const matchingCount = await page.evaluate(() => {
  const text = document.querySelector('.main-head .dim.small')?.textContent ?? ''
  return Number(text.replace(/\D/g, ''))
})
const spacerHeight = await page.locator('.table-body > div').evaluate((el) => parseFloat(el.style.height))
check(
  Math.abs(spacerHeight / 28 - matchingCount) < 1,
  'scroll height represents the full result set',
  `${Math.round(spacerHeight / 28)} rows for ${matchingCount} matches`
)
check((await rowCount()) < 100, 'DOM rows stay bounded by the virtual window', `${await rowCount()} rows`)

// --- clicking a row selects and activates it
await page.locator('.row[data-path]').first().click()
await page.waitForTimeout(350)
check((await page.locator('.row.active[data-path]').count()) === 1, 'clicking a row makes it active')
check((await page.locator('.row.selected[data-path]').count()) === 1, 'clicking a row selects it')

// --- Ctrl+click adds, Shift+click extends
await page.locator('.row[data-path]').nth(2).click({ modifiers: ['Control'] })
await page.waitForTimeout(250)
check((await page.locator('.row.selected[data-path]').count()) === 2, 'Ctrl+click extends the selection')
await page.locator('.row[data-path]').nth(5).click({ modifiers: ['Shift'] })
await page.waitForTimeout(250)
const shiftCount = await page.locator('.row.selected[data-path]').count()
check(shiftCount >= 4, 'Shift+click selects a range', `${shiftCount} rows`)

// --- the footer and status bar report the selection
const status = (await page.locator('.statusbar').textContent()) ?? ''
check(/\d+ selected/.test(status), 'the status bar reports the selection', status.trim())

// --- sorting: click a header twice, expect the arrow to flip
const sortBefore = await page.locator('.head-cell .sort-arrow').first().textContent()
await page.locator('.head-cell[data-sort-key="durationS"]').click()
await page.waitForFunction(() => {
  const table = document.querySelector('.table')
  return table?.dataset.querying === 'false' && table.dataset.epoch === table.dataset.segmentsEpoch
}, undefined, { timeout: 20_000 })
const lengthArrow = await page
  .locator('.head-cell[data-sort-key="durationS"]')
  .locator('.sort-arrow')
  .textContent()
check(!!lengthArrow, 'clicking Length sorts by it', lengthArrow)
await page.locator('.head-cell[data-sort-key="durationS"]').click()
await page.waitForFunction(() => {
  const table = document.querySelector('.table')
  return table?.dataset.querying === 'false' && table.dataset.epoch === table.dataset.segmentsEpoch
}, undefined, { timeout: 20_000 })
const flipped = await page
  .locator('.head-cell[data-sort-key="durationS"]')
  .locator('.sort-arrow')
  .textContent()
check(
  lengthArrow !== flipped,
  'clicking the same header flips the direction',
  `${lengthArrow} → ${flipped}`
)
check(!!sortBefore, 'the default sort shows an arrow', sortBefore)

// --- search filters the list, and clearing restores it
await search('kick')
const kickCount = await rowCount()
check(kickCount > 0, 'searching "kick" finds rows', `${kickCount}`)
const matchLabel = (await page.locator('.main-head .dim.small').textContent()) ?? ''
check(/matches/.test(matchLabel), 'the header reports the full match count', matchLabel.trim())
await search('zzzznotasoundzzz')
check((await rowCount()) === 0, 'a query with no hits empties the list')
check((await page.locator('.table-empty').count()) === 1, 'an empty result shows the empty state')
await search('')
await page.waitForTimeout(400)
check((await rowCount()) > 0, 'clearing the query restores visible rows')
const restoredMatchLabel = (await page.locator('.main-head .dim.small').textContent()) ?? ''
check(/282,762/.test(restoredMatchLabel), 'clearing the query restores the full result count', restoredMatchLabel)

// Scroll across sparse windows and verify each target window is populated while
// the segment LRU keeps renderer heap growth bounded.
const heapSession = await page.context().newCDPSession(page)
const heapUsed = async () => {
  await heapSession.send('HeapProfiler.collectGarbage')
  const usage = await heapSession.send('Runtime.getHeapUsage')
  return usage.usedSize
}
const heapBeforeScroll = await heapUsed()
for (let step = 1; step <= 20; step++) {
  const target = Math.floor((matchingCount - 1) * step / 20)
  await page.evaluate((target) => {
    const el = document.querySelector('.table-body')
    el.scrollTop = target * 28
    el.dispatchEvent(new Event('scroll'))
  }, target)
  await page.waitForFunction(
    (target) => [...document.querySelectorAll('.row[data-path]')].some((row) => {
      const index = Number(row.getAttribute('aria-posinset')) - 1
      return index >= target - 40 && index <= target + 70
    }),
    target,
    { timeout: 10_000 }
  )
}
await page.evaluate(() => {
  const el = document.querySelector('.table-body')
  el.scrollTop = 0
  el.dispatchEvent(new Event('scroll'))
})
const heapAfterScroll = await heapUsed()
if (heapBeforeScroll !== null && heapAfterScroll !== null) {
  const growth = heapAfterScroll - heapBeforeScroll
  check(growth < 64 * 1024 * 1024, 'scrolling does not grow renderer heap without bound', `${Math.round(growth / 1024 / 1024)} MiB`)
}
await heapSession.detach()

// Home/End navigate by absolute index, fetching an unloaded window when needed.
await page.evaluate(() => document.activeElement?.blur())
await page.keyboard.press('Home')
await page.waitForFunction(
  () => Number(document.querySelector('.row.active[data-path]')?.getAttribute('aria-posinset')) === 1,
  undefined,
  { timeout: 10_000 }
)
check(true, 'Home fetches and activates the first virtual row')
await page.keyboard.press('End')
await page.waitForFunction(
  (count) => {
    const row = document.querySelector('.row.active[data-path]')
    return Number(row?.getAttribute('aria-posinset')) === count
  },
  matchingCount,
  { timeout: 20_000 }
)
check(true, 'End fetches and activates the final virtual row', `${matchingCount}`)
await page.keyboard.press('Control+a')
await page.waitForFunction(
  (count) => document.querySelector('.statusbar')?.textContent?.includes(`${count.toLocaleString()} selected`),
  matchingCount,
  { timeout: 5_000 }
)
check(true, 'Ctrl+A selects the filter without transferring paths', `${matchingCount}`)

// --- the row context menu
await page.locator('.row[data-path]').first().click({ button: 'right' })
await page.waitForTimeout(300)
const menuItems = await page.locator('.menu-item').count()
check(menuItems >= 6, 'right-click opens the row menu', `${menuItems} items`)
check(
  (await page.locator('.menu-item[data-menu-id="category"]').count()) === 1,
  'the menu offers Set Category'
)
await page.keyboard.press('Escape')
await page.waitForTimeout(200)
check((await page.locator('.menu').count()) === 0, 'Escape closes the menu')

// --- the category dialog is an iX modal
await page.locator('.row[data-path]').first().click({ button: 'right' })
await page.locator('.menu-item[data-menu-id="category"]').click()
await page.waitForTimeout(900)
check((await page.locator('ix-modal').count()) === 1, 'Set Category opens an iX modal')
// The host is display:none until iX adds `.visible`, and the dialog inside it is
// `position: fixed`, so the host itself measures 0×0 even when the modal is up.
// The thing to measure is the dialog in the shadow root.
const modalState = await page.evaluate(() => {
  const m = document.querySelector('ix-modal')
  if (!m) return { visible: false, w: 0, h: 0 }
  const dlg = m.shadowRoot?.querySelector('dialog')
  const r = dlg?.getBoundingClientRect()
  return { visible: m.classList.contains('visible'), w: Math.round(r?.width ?? 0), h: Math.round(r?.height ?? 0) }
})
check(modalState.visible, 'the modal host is marked visible')
check(modalState.w > 300 && modalState.h > 100, 'the modal dialog has a real size', `${modalState.w}×${modalState.h}`)

// Escape must close it: iX listens for `cancel` on the inner dialog and animates
// out, so the assertion waits for the class rather than for the element to vanish.
await page.keyboard.press('Escape')
await page.waitForTimeout(900)
const closed = await page.evaluate(() => {
  const m = document.querySelector('ix-modal')
  return m ? !m.classList.contains('visible') : true
})
check(closed, 'Escape closes the modal')

// --- AUTO PLAY is a word, not a glyph, and it toggles
const autoText = (await page.locator('#auto-play').textContent())?.trim()
check(autoText === 'AUTO PLAY', 'the toggle is labelled in words', autoText)

// iX components render their control inside a shadow root, so a `.click()` on the
// host does nothing — the inner button is the thing a user clicks. Driving it
// directly is what the browser does on a real click.
const clickInner = (selector) =>
  page.evaluate((selector) => {
    const host = document.querySelector(selector)
    const inner = host?.shadowRoot?.querySelector('button')
    if (!inner) return 'no inner button'
    inner.click()
    return 'ok'
  }, selector)

const pressedBefore = await page.evaluate(() => document.querySelector('#auto-play').pressed)
check((await clickInner('#auto-play')) === 'ok', 'AUTO PLAY has a clickable inner button')
await page.waitForTimeout(400)
const pressedAfter = await page.evaluate(() => document.querySelector('#auto-play').pressed)
check(
  pressedBefore !== pressedAfter,
  'clicking AUTO PLAY flips it',
  `${pressedBefore} → ${pressedAfter}`
)
check(
  (await page.evaluate(() => localStorage.getItem('mio:settings'))) ===
    JSON.stringify({
      autoPlay: pressedAfter,
      showCategory: true
    }),
  'the toggle is persisted'
)
// Put it back on, so the rest of the checks audition rows.
if (!pressedAfter) {
  await clickInner('#auto-play')
  await page.waitForTimeout(350)
}

// --- transport. The playhead can only be watched on a clip long enough to
// outlast two polls, so the longest loaded row is chosen from the rendered
// durations rather than by hoping a sort put it first.
// The sort checks above left Length ascending, which puts the shortest clips on
// screen. Click it once more for descending, so the longest loaded row is visible.
const lengthArrowNow = await page
  .locator('.head-cell[data-sort-key="durationS"]')
  .locator('.sort-arrow')
  .textContent()
if (lengthArrowNow === '▲') {
  await page.locator('.head-cell[data-sort-key="durationS"]').click()
  await page.waitForFunction(() => {
    const table = document.querySelector('.table')
    return table?.dataset.querying === 'false' && table.dataset.epoch === table.dataset.segmentsEpoch
  }, undefined, { timeout: 20_000 })
}
const longestIndex = await page.evaluate(() => {
  const parse = (text) => {
    const m = /^(\d+):(\d+(?:\.\d+)?)$/.exec((text ?? '').trim())
    return m ? Number(m[1]) * 60 + Number(m[2]) : 0
  }
  const rows = [...document.querySelectorAll('.row[data-path]')]
  let best = 0
  let bestLen = -1
  rows.forEach((row, i) => {
    // Columns: ★ · Name · Category · Length · Ch · Rate · Size
    const cell = row.children[3]
    const len = parse(cell?.textContent)
    if (len > bestLen) {
      bestLen = len
      best = i
    }
  })
  return { index: best, length: bestLen }
})
check(longestIndex.length > 3, 'the list holds a clip long enough to observe', `${longestIndex.length.toFixed(2)}s`)

const longestRow = page.locator('.row[data-path]').nth(longestIndex.index)
const longestName = ((await longestRow.locator('.cell.name').textContent()) ?? '').trim()
await longestRow.click()
// Wait for the dock to actually be showing this clip. `.wave-time` appears for the
// previous row too, so waiting on its presence alone reads the old clip's position
// and compares two different clips.
await page.waitForFunction(
  (name) => document.querySelector('.dock-name')?.textContent?.trim() === name,
  longestName,
  { timeout: 10_000 }
)
await page.waitForTimeout(500)

const position = async () =>
  page.evaluate(() => {
    const label = document.querySelector('.wave-time')?.textContent ?? ''
    const [now] = label.split('/')
    const [mm, ss] = (now ?? '0:00').split(':')
    return Number(mm) * 60 + Number(ss)
  })
// The playhead is polled once per animation frame, so for a moment after a publish
// it still reports the previous clip's position. Wait for it to be inside the new
// clip's duration before taking the first sample, otherwise the "before" reading
// belongs to a different clip and the comparison is meaningless.
await page.waitForFunction(
  () => {
    const label = document.querySelector('.wave-time')?.textContent ?? ''
    const [now, total] = label.split('/')
    const parse = (text) => {
      const m = /^(\d+):(\d+(?:\.\d+)?)$/.exec((text ?? '').trim())
      return m ? Number(m[1]) * 60 + Number(m[2]) : 0
    }
    return parse(now) < parse(total)
  },
  undefined,
  { timeout: 5000 }
)
const p1 = await position()
await page.waitForTimeout(900)
const p2 = await position()
check(p2 > p1, 'the playhead advances while playing', `${p1}s → ${p2}s`)

// The transport buttons are disabled with nothing loaded; iX sets
// `pointer-events: none` on a disabled host, so Playwright refuses to click. The
// state is asserted first, then the click goes through the inner button.
const toggleDisabled = await page.evaluate(() =>
  document.querySelector('#transport-toggle').classList.contains('disabled')
)
check(!toggleDisabled, 'the pause button is enabled while playing')

await clickInner('#transport-toggle')
await page.waitForTimeout(600)
// iX moves the host's `aria-label` onto its inner button and removes it from the
// host, so the label has to be read where a screen reader reads it.
const pausedLabel = await page.evaluate(() => {
  const host = document.querySelector('#transport-toggle')
  return host?.shadowRoot?.querySelector('button')?.getAttribute('aria-label') ?? ''
})
check(pausedLabel === 'Resume', 'the transport button switches to Resume when paused', pausedLabel)
const p3 = await position()
await page.waitForTimeout(800)
const p4 = await position()
check(Math.abs(p4 - p3) < 0.3, 'the playhead holds while paused', `${p3}s → ${p4}s`)

await clickInner('#transport-stop')
await page.waitForTimeout(500)
const stopped = await position()
check(stopped < 0.2, 'Stop clears the playhead', `${stopped}s`)

// --- seeking on the waveform, on the same long clip
await longestRow.click()
await page.waitForFunction(
  (name) => document.querySelector('.dock-name')?.textContent?.trim() === name,
  longestName,
  { timeout: 10_000 }
)
await page.waitForTimeout(700)
const box = await page.locator('canvas.wave').boundingBox()
if (box) {
  await page.mouse.click(box.x + box.width * 0.7, box.y + box.height / 2)
  await page.waitForTimeout(700)
  const seeked = await position()
  const duration = await page.evaluate(() => {
    const label = document.querySelector('.wave-time')?.textContent ?? ''
    const [, after] = label.split('/')
    const [mm, ss] = (after ?? '0:00').trim().split(':')
    return Number(mm) * 60 + Number(ss)
  })
  check(
    seeked > duration * 0.5,
    'clicking the waveform seeks past the halfway point',
    `${seeked.toFixed(2)}s of ${duration.toFixed(2)}s`
  )
  // The seek target is ~70% of the file, far beyond any window decoded so
  // far. With the window chain the playhead must land there and keep going:
  // one second later it reads at or beyond the target, which only holds if
  // the seek-window decode and the queued continuation both happened.
  await page.waitForTimeout(1_200)
  const after = await position()
  check(
    after >= seeked - 0.25,
    'playback continues forward from the seek target (window chain)',
    `${after.toFixed(2)}s after seeking to ${seeked.toFixed(2)}s`
  )
} else {
  check(false, 'the waveform canvas exists')
}

// --- the settings dialog opens and shows the theme picker
await page.evaluate(() => {
  const host = document.querySelector('#settings-button')
  host?.shadowRoot?.querySelector('button')?.click()
})
await page.waitForTimeout(900)
check((await page.locator('ix-modal').count()) >= 1, 'Settings opens an iX modal')
// The settings modal is the one carrying the theme cards: a closing dialog from
// the previous step can still be in the DOM for a frame, and reading the first
// `ix-modal` would report that one.
const settingsOpen = await page.evaluate(() => {
  const m = [...document.querySelectorAll('ix-modal')].find((el) => el.querySelector('.theme-card'))
  if (!m) return false
  const dlg = m.shadowRoot?.querySelector('dialog')
  return m.classList.contains('visible') && !!dlg?.open
})
check(settingsOpen, 'the settings modal is visible')
const themeCards = await page.locator('.theme-card').count()
check(themeCards >= 4, 'the settings dialog lists the built-in themes', `${themeCards} cards`)
await page.keyboard.press('Escape')
await page.waitForTimeout(500)

if (problems.length === 0) {
  console.log('\ninteractions: OK')
} else {
  console.log('\ninteraction problems:')
  for (const p of problems) console.log(`  - ${p}`)
}
await browser.close()
process.exit(problems.length === 0 ? 0 : 1)
