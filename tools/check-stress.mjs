// On-demand stress checks for search terms, virtual scrolling, and audition churn.
//
//   node tools/check-stress.mjs [port]
//
// Run when frontend/backend data structures or algorithms change. This is not
// part of the routine feature-check loop; it deliberately bursts requests at the
// real window and engine to catch stale results, stuck windows, and decode races.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

const pageErrors = []
page.on('pageerror', (error) => pageErrors.push(error.message))
await page.evaluate(() => {
  localStorage.setItem('mio:locale', 'en')
  localStorage.setItem('mio:settings', JSON.stringify({ autoPlay: true, showCategory: true }))
})
await page.reload()
await page.waitForSelector('.row[data-path]', { timeout: 90_000 })
await page.waitForFunction(
  () => document.querySelector('.table')?.dataset.querying === 'false',
  undefined,
  { timeout: 30_000 }
)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}
const waitSettled = () =>
  page.waitForFunction(
    () => {
      const table = document.querySelector('.table')
      return table?.dataset.querying === 'false' && table.dataset.epoch === table.dataset.segmentsEpoch
    },
    undefined,
    { timeout: 30_000 }
  )
const addTerm = (term) =>
  page.evaluate((value) => {
    const input = document.querySelector('#search-input')
    input.value = value
    input.dispatchEvent(new CustomEvent('valueChange', { detail: value, bubbles: true }))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, composed: true }))
  }, term)
const matching = async () =>
  page.evaluate(() => {
    const text = document.querySelector('#match-count')?.textContent ?? ''
    return Number(text.replace(/\D/g, '')) || 0
  })
const rowCount = () => page.locator('.row[data-path]').count()
const invoke = (command, args = {}) =>
  page.evaluate(([name, payload]) => window.__TAURI_INTERNALS__.invoke(name, payload), [command, args])

// Burst Enter commits. These conditions intentionally produce an arbitrary final
// result count; compare the UI with an independent count from the same IPC contract.
const terms = ['kick', 'snare', 'hat', 'tom', 'clap', 'kikc', 'zzqv-no-hit', 'AND OR *']
await page.evaluate((values) => {
  const input = document.querySelector('#search-input')
  for (const value of values) {
    input.value = value
    input.dispatchEvent(new CustomEvent('valueChange', { detail: value, bubbles: true }))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, composed: true }))
  }
}, terms)
await waitSettled()
check((await page.locator('.query-chip').count()) === terms.length, 'rapid term commits retain every chip')
const termQuery = terms.join(' ')
const independentCount = await invoke('count_matching', { request: { query: termQuery } })
check((await matching()) === independentCount, 'burst results agree with backend count', `${await matching()} / ${independentCount}`)

// Remove most terms without waiting between clicks, then leave one known hit.
for (const term of terms.slice(1)) await page.locator('.query-chip').filter({ hasText: term }).click()
await waitSettled()
check((await page.locator('.query-chip').count()) === 1, 'rapid chip removal leaves only the retained term')
check((await rowCount()) > 0, 'removing false-positive filters restores rows')

// A typo/no-hit term narrows the result to empty; removing just that term recovers.
await addTerm('kikc')
await waitSettled()
check((await rowCount()) === 0, 'misspelled term produces an empty result')
check((await page.locator('.table-empty').count()) === 1, 'empty result state remains visible')
await page.locator('.query-chip').filter({ hasText: 'kikc' }).click()
await waitSettled()
await page.waitForSelector('.row[data-path]', { timeout: 10_000 })
check((await rowCount()) > 0, 'removing typo recovers the valid search')

// Rapidly clear the remaining search condition with Escape.
await page.locator('#search-input').evaluate((input) =>
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, composed: true }))
)
await waitSettled()
check((await page.locator('.query-chip').count()) === 0, 'Escape clears committed search terms')

// Alternate deep scrollbar jumps faster than requests can finish. The final
// viewport must resolve to real rows rather than remaining on skeletons.
const totalMatches = await matching()
const jumps = Array.from({ length: 36 }, (_, i) =>
  Math.round(Math.max(0, totalMatches - 30) * (i % 2 === 0 ? 0.92 : 0.03))
)
for (const index of jumps) {
  await page.evaluate((target) => {
    const body = document.querySelector('.table-body')
    body.scrollTop = target * 28
    body.dispatchEvent(new Event('scroll'))
  }, index)
}
const finalScroll = jumps.at(-1)
await page.waitForFunction(
  (target) =>
    [...document.querySelectorAll('.table-body .row:not(.skeleton)')].some(
      (row) => Math.abs(Number(row.dataset.index) - target) < 40
    ),
  finalScroll,
  { timeout: 20_000 }
)
check((await page.locator('.table-body .row:not(.skeleton)').count()) >= 5, 'rapid scroll repaints a populated viewport')
check((await rowCount()) < 100, 'virtualized DOM remains bounded during scroll')

// Rapid audition burst stresses overlapping decode/publish requests. Clicking a
// row activates it synchronously; after all clicks, the last target must win.
await page.evaluate(() => {
  const body = document.querySelector('.table-body')
  body.scrollTop = 0
  body.dispatchEvent(new Event('scroll'))
})
await page.waitForFunction(() => document.querySelectorAll('.row[data-path]').length >= 12, undefined, { timeout: 15_000 })
const auditionRows = page.locator('.row[data-path]')
const auditionCount = Math.min(20, await auditionRows.count())
let lastName = ''
for (let i = 0; i < auditionCount; i++) {
  const row = auditionRows.nth(i)
  // The name cell may append a shared-description count (for example `x428`),
  // while the dock intentionally shows only the real filename.
  const path = await row.getAttribute('data-path')
  lastName = path?.split(/[\\/]/).pop()?.trim() ?? ''
  await row.click()
}
await page.waitForFunction((name) => document.querySelector('.dock-name')?.textContent?.trim() === name, lastName, { timeout: 20_000 })
check((await page.locator('.row.active[data-path]').count()) === 1, 'rapid audition leaves one active row')
check((await page.locator('.wave-overlay.bad').count()) === 0, 'rapid audition leaves no decode error')
const playback = await invoke('get_playback_position')
check(typeof playback.playing === 'boolean', 'playback IPC still responds after audition burst')

check(pageErrors.length === 0, 'no uncaught frontend errors', pageErrors.slice(0, 3).join(' | '))
if (problems.length === 0) console.log('\nstress: OK')
else {
  console.log('\nstress problems:')
  for (const problem of problems) console.log(`  - ${problem}`)
}
await browser.close()
process.exit(problems.length === 0 ? 0 : 1)
