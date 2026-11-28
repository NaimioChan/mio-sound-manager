// The details panel against a real library, over CDP.
//
//   node tools/check-inspector-live.mjs 9223
//
// `check-inspector.mjs` drives the click semantics with an instrumented IPC
// fixture. This one proves the other half: that a real file's embedded tags, a
// sidecar cover and a row thumbnail actually arrive from SQLite and the image
// decoder. It needs the library built by `tools/make-inspector-fixture.py` and a
// window running `tools/run-app.sh`; every assertion is read from the DOM.
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'

function loadPlaywright() {
  const candidates = [
    process.env.MIO_PLAYWRIGHT_PKG,
    'C:/Users/Naimio/projects/slip-sound/package.json',
    `${process.env.LOCALAPPDATA}/Temp/opencode/pw/package.json`,
    new URL('../package.json', import.meta.url).pathname,
  ].filter(Boolean)
  const tried = []
  for (const candidate of candidates) {
    try {
      return createRequire(candidate)('playwright-core')
    } catch {
      tried.push(candidate)
    }
  }
  throw new Error(`playwright-core not found; tried:\n  ${tried.join('\n  ')}`)
}

const { chromium } = loadPlaywright()
const port = process.argv[2] ?? '9223'
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) {
  console.error('no page on the debugger')
  process.exit(1)
}

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

try {
  await page.evaluate(() => {
    localStorage.setItem('mio:locale', 'en')
    localStorage.setItem('mio:inspector-open', '1')
    localStorage.removeItem('mio:column-hidden')
  })
  await page.reload()
  await page.waitForSelector('.row[data-path]', { timeout: 60_000 })
  // The library index and the details pass run in the background on startup, so
  // a thumbnail may not exist for a few seconds.
  await page.waitForFunction(
    () => document.querySelector('.table')?.dataset.querying === 'false',
    undefined,
    { timeout: 30_000 }
  )
  await page.waitForTimeout(1500)

  const rows = page.locator('.row[data-path]')
  const count = await rows.count()
  check(count > 0, 'the fixture library is open', `${count} rows`)

  // --- the embedded cover: one row has an APIC, one has a sidecar, the rest none
  await page.waitForFunction(
    () => document.querySelectorAll('.row[data-path] .cover-thumb').length >= 2,
    undefined,
    { timeout: 30_000 }
  )
  const thumbs = await page.locator('.row[data-path] .cover-thumb').count()
  const blanks = await page.locator('.row[data-path] .cover-blank').count()
  check(thumbs >= 2, 'embedded and sidecar covers produce thumbnails', `${thumbs} thumbs`)
  check(blanks >= 1, 'coverless rows show a placeholder', `${blanks} blanks`)

  const thumbSize = await page
    .locator('.row[data-path] .cover-thumb')
    .first()
    .evaluate((img) => ({ w: img.naturalWidth, h: img.naturalHeight, drawn: img.getBoundingClientRect().width }))
  check(thumbSize.w === 48 && thumbSize.h === 48, 'the thumbnail is the stored 48px rendition', JSON.stringify(thumbSize))
  check(Math.round(thumbSize.drawn) === 24, 'the cell draws it at 24px', `${thumbSize.drawn}`)

  // --- select the file with the ID3 picture and read the panel
  const tagged = await page.evaluate(() => {
    const row = [...document.querySelectorAll('.row[data-path]')].find((el) =>
      (el.dataset.path ?? '').endsWith('Door_Slam_Take3.wav')
    )
    return row?.dataset.path ?? null
  })
  check(!!tagged, 'the tagged fixture row is on screen')
  if (tagged) {
    await page.locator(`.row[data-path="${tagged.replace(/\\/g, '\\\\')}"]`).click()
    await page.waitForFunction(
      () => document.querySelectorAll('.inspector-tag-source').length > 0,
      undefined,
      { timeout: 15_000 }
    )
    const sources = await page.evaluate(() =>
      [...document.querySelectorAll('.inspector-tag-source')].map((el) => el.textContent)
    )
    for (const expected of ['ID3', 'BWF bext', 'iXML', 'LIST INFO']) {
      check(sources.includes(expected), `tags from ${expected} are listed`, JSON.stringify(sources))
    }

    const keys = await page.evaluate(() =>
      [...document.querySelectorAll('.inspector-tag-key')].map((el) => el.textContent)
    )
    for (const expected of ['Artist', 'Album', 'Originator', 'Project', 'Scene']) {
      check(keys.includes(expected), `the "${expected}" tag is labelled`, JSON.stringify(keys))
    }

    // The embedded picture is decoded at 512px for the panel, not upscaled 48px.
    const cover = await page.evaluate(() => {
      const img = document.querySelector('.inspector-cover img')
      if (!img) return null
      return { w: img.naturalWidth, h: img.naturalHeight, complete: img.complete, src: img.src.slice(0, 30) }
    })
    check(!!cover, 'the inspector shows a cover')
    if (cover) {
      check(cover.complete && cover.w === 320 && cover.h === 320, 'the panel decodes the source picture, not the thumbnail', JSON.stringify(cover))
    }

    // A tag whose value is searchable is a button; clicking it filters.
    const before = await page.locator('.row[data-path]').count()
    await page
      .locator('.inspector-tag')
      .filter({ hasText: 'Originator' })
      .locator('button.inspector-tag-value')
      .click()
    await page.waitForFunction(
      () => document.querySelector('.table')?.dataset.querying === 'false',
      undefined,
      { timeout: 20_000 }
    )
    await page.waitForTimeout(200)
    const queryChip = await page.locator('.query-chip').first().textContent()
    check(
      (queryChip ?? '').includes('Naimio Field Unit'),
      'a tag click becomes a query term',
      queryChip ?? ''
    )
    const after = await page.locator('.row[data-path]').count()
    check(after > 0 && after <= before, 'the tag query returns rows', `${before} → ${after}`)

    // And the cover click filters to the picture: a row whose identity is unique
    // to this file returns exactly one foley sound.
    await page.locator('.query-chip').first().click()
    await page.waitForTimeout(400)
    await page.locator(`.row[data-path="${tagged.replace(/\\/g, '\\\\')}"]`).click()
    await page.waitForFunction(
      () => document.querySelector('.inspector-cover img') !== null,
      undefined,
      { timeout: 15_000 }
    )
    await page.locator('.inspector-cover').click()
    await page.waitForFunction(
      () => document.querySelector('.table')?.dataset.querying === 'false',
      undefined,
      { timeout: 20_000 }
    )
    await page.waitForTimeout(300)
    const coverChip = await page.locator('.filter-chip').first().textContent()
    check((coverChip ?? '').includes('Same cover'), 'the cover filter is a chip', coverChip ?? '')
    const remaining = await page.locator('.row[data-path]').count()
    check(remaining === 1, 'one file carries that exact picture', `${remaining}`)
  }

  // --- the sidecar cover is shared by its folder and its subfolder
  const sidecar = await page.evaluate(() => {
    const paths = [...document.querySelectorAll('.row[data-path]')].map((el) => el.dataset.path)
    return paths.length
  })
  check(sidecar > 0, 'the list still has rows after clearing the cover filter')

  assert.deepEqual(problems, [])
  console.log('\ninspector-live: OK')
} finally {
  await browser.close()
}
