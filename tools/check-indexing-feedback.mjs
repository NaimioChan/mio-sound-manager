// Run against a desktop test session with the large benchmark library active.
// node tools/check-indexing-feedback.mjs [port] [scratch-parent] [expected-library] [--measure-metadata]
import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.argv[2] ?? '9223'}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
const invoke = (command, args) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args }
)
const active = async () => (await invoke('list_libraries')).find((library) => library.active)
let fixture
let originalRoot

try {
  await page.waitForSelector('.row[data-path]', { timeout: 30_000 })
  originalRoot = (await active()).root
  if (process.argv[4]) assert.equal(originalRoot, process.argv[4])
  const start = performance.now()
  let state = await active()
  if (process.argv.includes('--measure-metadata')) console.log('initial library status:', JSON.stringify(state))
  if (!process.argv.includes('--while-background-indexing')) {
    while (state.indexing || state.metadataIndexing) {
      assert.ok(performance.now() - start < 900_000, 'benchmark library did not settle')
      await delay(500)
      state = await active()
    }
    if (process.argv.includes('--measure-metadata')) {
      console.log(`settled after ${Math.round(performance.now() - start)}ms from first sample:`, JSON.stringify(state))
    }
  }
  await page.evaluate(() => localStorage.setItem('mio:locale', 'zh'))
  // This script assumes all eleven columns are visible to index header cells by key
  // position; a hidden column from an earlier run would make the failures look like
  // an index bug.
  await page.evaluate(() => {
    localStorage.removeItem('mio:column-hidden')
    localStorage.removeItem('mio:column-widths')
    localStorage.removeItem('mio:column-order')
  })
  await page.reload()
  await page.waitForSelector('.row[data-path]', { timeout: 30_000 })
  await page.setViewportSize({ width: 1280, height: 760 })

  assert.doesNotMatch(await page.locator('.statusbar').innerText(), /引擎周期/)
  await page.locator('#settings-button').click()
  await page.waitForSelector('.modal-form')
  assert.doesNotMatch(await page.locator('.modal-form').innerText(), /引擎周期/)
  await page.locator('.modal-actions ix-button').last().click()
  await page.locator('.modal-form').waitFor({ state: 'hidden' })
  console.log('engine period: absent from statusbar and settings')

  for (const width of [1280, 1024, 800, 680]) {
    await page.setViewportSize({ width, height: 760 })
    const headers = await page.locator('.head-cell').evaluateAll((cells) => cells.map((cell) => {
      const bounds = cell.getBoundingClientRect()
      const head = cell.parentElement.getBoundingClientRect()
      const text = document.createRange()
      text.selectNodeContents(cell)
      return {
        key: cell.dataset.sortKey,
        nowrap: getComputedStyle(cell).whiteSpace === 'nowrap',
        withinHeader: bounds.top >= head.top - 1 && bounds.bottom <= head.bottom + 1,
        textWithinRow: [...text.getClientRects()].every((r) => r.top >= bounds.top - 1 && r.bottom <= bounds.bottom + 1),
      }
    }))
    assert.ok(headers.some(({ key }) => key === 'ucsSubcategory'))
    assert.ok(headers.every((h) => h.nowrap && h.withinHeader && h.textWithinRow), JSON.stringify({ width, headers }))
  }
  console.log('header geometry: 1280/1024/800/680px')
  await page.setViewportSize({ width: 1280, height: 760 })

  const count = await invoke('count_matching', { request: {} })
  assert.ok(count >= 100_000, `expected large library, got ${count} rows`)
  for (const [key, sortBy] of [
    ['description', 'description'], ['description', 'description'], ['ucsSubcategory', 'ucs_subcategory'],
    ['ucsSubcategory', 'ucs_subcategory'], ['description', 'description'],
  ]) {
    const header = page.locator(`[data-sort-key="${key}"]`)
    await header.click()
    const arrow = await header.locator('.sort-arrow').textContent()
    const sortDir = arrow.includes('▲') ? 'asc' : 'desc'
    const checkAt = async (fraction, burst) => {
      const offset = await page.locator('.table-body').evaluate(async (body, { fraction, burst }) => {
        const jump = (f) => { body.scrollTop = Math.round((body.scrollHeight - body.clientHeight) * f) }
        if (burst) {
          for (const f of [0.95, 0.1, 0.8, 0.3]) {
            jump(f)
            await new Promise(requestAnimationFrame)
          }
        }
        jump(fraction)
        const rowHeight = parseFloat(getComputedStyle(body).getPropertyValue('--mio-row-h')) || 28
        return Math.ceil(body.scrollTop / rowHeight)
      }, { fraction, burst })
      const start = performance.now()
      const row = page.locator(`.table-body .row[data-index="${offset}"][data-path]`)
      await row.waitFor({ timeout: 5000 })
      const rendered = await row.getAttribute('data-path')
      const elapsed = Math.round(performance.now() - start)
      const [expected] = await invoke('search', { request: { sortBy, sortDir, limit: 1, offset } })
      assert.equal(rendered, expected.path, `${sortBy}/${sortDir} at ${fraction}: stale segment`)
      return elapsed
    }
    const times = []
    for (const fraction of [0.1, 0.3, 0.5, 0.8, 0.95, 0.3]) times.push(await checkAt(fraction, fraction === 0.3))
    console.log(`${sortBy}/${sortDir}: correct paths at all depths, max ${Math.max(...times)}ms`)
  }

  fixture = await mkdtemp(join(process.argv[3] ?? tmpdir(), 'mio-index-feedback-'))
  const wav = Buffer.from('524946462600000057415645666d7420100000000100010080bb0000007701000200100064617461020000000000', 'hex')
  await writeFile(join(fixture, 'initial.wav'), wav)
  await page.evaluate(async (root) => {
    const callback = window.__TAURI_INTERNALS__.transformCallback(() => {})
    await window.__TAURI_INTERNALS__.invoke('add_library', { root, full: true, onProgress: `__CHANNEL__:${callback}` })
  }, fixture)
  await page.reload()
  await page.locator('.lib-group.active .lib-head').waitFor()
  for (let start = 0; start < 12_000; start += 250) {
    await Promise.all(Array.from({ length: 250 }, (_, i) => writeFile(join(fixture, `sound_${start + i}.wav`), wav)))
  }

  // Observe the real UI while its context-menu command is running. Record
  // mutation batches (not timer snapshots), including very short phases.
  await page.evaluate(() => {
    window.__indexFeedback = []
    const observer = new MutationObserver(() => {
      const overlay = document.querySelector('.index-overlay')
      if (!overlay) return
      const sample = {
        phase: overlay.dataset.phase,
        indeterminate: !!overlay.querySelector('progress:indeterminate'),
        detail: overlay.querySelector('.index-detail').textContent.trim().replace(/\s+/g, ' '),
      }
      if (JSON.stringify(window.__indexFeedback.at(-1)) !== JSON.stringify(sample)) {
        window.__indexFeedback.push(sample)
      }
    })
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true })
  })
  await page.locator('.lib-group.active .lib-head').click({ button: 'right' })
  await page.locator('[data-menu-id="reindex-full"]').click()
  await page.waitForFunction(() => window.__indexFeedback.some((sample) => sample.phase === 'Indexing'))
  await page.locator('.index-overlay').waitFor({ state: 'detached', timeout: 60_000 })
  const samples = await page.evaluate(() => window.__indexFeedback)
  for (const phase of ['Scanning', 'Comparing', 'Finalizing']) {
    assert.ok(samples.some((sample) => sample.phase === phase && sample.indeterminate), JSON.stringify(samples))
  }
  const indexing = samples.filter((sample) => sample.phase === 'Indexing' && !sample.indeterminate)
  assert.ok(new Set(indexing.map((sample) => sample.detail)).size > 1, JSON.stringify(samples))
  console.log(`overlay: ${[...new Set(samples.map((sample) => sample.phase))].join(' → ')}; ${indexing.length} determinate updates`)

  const progress = page.locator('.statusbar .metadata-progress')
  await progress.waitFor({ timeout: 10_000 })
  await page.waitForFunction(() => {
    return document.querySelector('.metadata-progress progress')?.value > 0
  })
  const label = await progress.evaluate((bar) => {
    const label = bar.querySelector('span')
    const track = bar.querySelector('progress')
    const center = (el) => { const r = el.getBoundingClientRect(); return r.top + r.height / 2 }
    return {
      label: label.textContent.trim(),
      counts: bar.textContent.match(/[\d,]+\s*\/\s*[\d,]+/g)?.length ?? 0,
      centerOffset: Math.abs(center(label) - center(track)),
    }
  })
  assert.match(label.label, /^正在读取嵌入式 metadata [\d,]+ \/ [\d,]+$/)
  assert.equal(label.counts, 1)
  assert.ok(label.centerOffset <= 1, `metadata track center offset ${label.centerOffset}px`)
  console.log(`metadata copy: ${label.label} (one counter, center offset ${label.centerOffset}px)`)
  assert.equal((await active()).metadataIndexing, true)
  await page.waitForSelector('.row[data-path]')
  assert.equal(await invoke('count_matching', { request: { query: 'sound' } }), 12_000)
  await progress.waitFor({ state: 'detached', timeout: 60_000 })
} finally {
  try {
    if (fixture) {
      await invoke('remove_library', { root: fixture })
      if (originalRoot) await invoke('set_active_library', { root: originalRoot })
      await rm(fixture, { recursive: true, force: true })
    }
  } finally {
    await browser.close()
  }
}
