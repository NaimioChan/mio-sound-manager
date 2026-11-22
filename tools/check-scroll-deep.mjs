// Scroll-freeze probe: point the window at the 280k library, jump the scrollbar
// deep into the list, and assert the viewport fills with real rows quickly.
//
//   node tools/check-scroll-deep.mjs [port] [library-root]
//
// The freeze this guards against was: scrollbar drag past ~5% of a 280k list
// left the table empty and the window unresponsive, because every paged fetch
// re-sorted the whole table on the main thread.

import { createRequire } from 'node:module'
import { setTimeout as sleep } from 'node:timers/promises'
import os from 'node:os'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const contexts = browser.contexts()
if (contexts.length === 0) throw new Error('no webview context')
const page = await contexts[0].pages()[0]
// Do not reload: the app restores its session and reload would re-run startup.
// Only pin the locale if it is not already English (the CDP checks assert on
// English copy; a Chinese or Japanese session would fail on missing locators).
const locale = await page.evaluate(() => localStorage.getItem('mio:locale'))
if (locale !== 'en') {
  await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
  await page.reload({ waitUntil: 'load' })
  await page.waitForFunction(() => document.querySelector('.table-body') !== null, { timeout: 20000 })
}

const listHeight = async () =>
  page.evaluate(() => {
    const list = document.querySelector('[role="list"]')
    return list ? list.getBoundingClientRect().height / 28 : 0
  })

let rows = await listHeight()
console.log(`rows in list: ${rows}`)
if (rows < 10000) {
  process.exit(2)
}

const body = await page.$('.table-body')

for (const frac of [0.1, 0.3, 0.5, 0.8, 0.95]) {
  const target = Math.floor((rows * 28 - 600) * frac)
  const t0 = Date.now()
  // Programmatic jump = scrollbar drag to a position.
  await body.evaluate((el, top) => { el.scrollTop = top }, target)
  // Deadline: 5s to paint real (non-skeleton) rows *at the new offset* — the
  // rows already on screen before the jump must not count as success.
  let ok = false
  let waitMs = 0
  while (Date.now() - t0 < 5000) {
    ok = await page.evaluate(
      ([want]) => {
        const painted = [...document.querySelectorAll('.table-body .row:not(.skeleton)')]
        return painted.length >= 5 && painted.some((r) => Number(r.dataset.index) >= want)
      },
      [Math.floor(target / 28)]
    )
    if (ok) break
    await sleep(100)
  }
  waitMs = Date.now() - t0
  const info = await page.evaluate(() => {
    const painted = [...document.querySelectorAll('.table-body .row:not(.skeleton)')]
      .map((r) => ({ i: Number(r.dataset.index), text: r.textContent.trim().slice(0, 36) }))
      .sort((a, b) => a.i - b.i)
    return { count: painted.length, firstIndex: painted[0]?.i ?? -1, first: painted[0]?.text ?? '' }
  })
  console.log(
    `jump to ${Math.round(frac * 100)}% (scrollTop ${target}): ${ok ? 'ok' : 'TIMEOUT'} in ${waitMs}ms, ${info.count} rows, firstIndex=${info.firstIndex} "${info.first}"`
  )
  if (!ok) {
    console.log(`FAIL: viewport never filled at ${frac}`)
    process.exit(1)
  }
}

// And the window must still respond: click the sort header after the jumps.
await page.click('[data-sort-key="filename"]')
await sleep(300)
const arrow = await page.evaluate(() => document.querySelector('[data-sort-key="filename"] .sort-arrow')?.textContent ?? '')
console.log(`header click after deep jumps: arrow=${arrow} — window responsive`)
console.log('scroll-deep: OK')
process.exit(0)
