// Run against the Vite renderer with an isolated IPC fixture:
// node tools/check-responsive-layout.mjs http://127.0.0.1:1420
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'
const browser = await chromium.launch(existsSync(edge) ? { executablePath: edge } : { channel: 'msedge' })
try {
  const page = await browser.newPage()
  page.on('pageerror', (error) => console.error(error))
  await page.addInitScript(() => {
    localStorage.setItem('mio:locale', 'zh')
    localStorage.setItem('mio:settings', JSON.stringify({ autoPlay: false }))
    localStorage.setItem('mio:sidebar-width', '520')
    // The inspector is a layout column now; a profile that left it open would
    // move the table's right edge and make the width checks read a different
    // layout than the one they describe.
    localStorage.removeItem('mio:inspector-open')
    localStorage.removeItem('mio:inspector-width')
    localStorage.removeItem('mio:column-hidden')
    localStorage.removeItem('mio:column-widths')
    localStorage.removeItem('mio:column-order')
    const root = 'C:\\layout-fixture'
    const rows = ['a.wav', 'b_' + 'LongFileName_'.repeat(12) + '.wav'].map((filename, i) => ({
      filename, path: `${root}\\${filename}`, durationS: 3, durationLabel: '0:03.00', channels: 2,
      sampleRate: 48000, filesize: 48000, favorite: false, category: 'Foley', matchedTerms: [], bpm: null,
      subcategory: i ? 'LongCategoryNameWithoutBreaks'.repeat(12) : null,
      description: i ? 'A long sound description with words and many details. '.repeat(12) : null,
      coverKey: null,
    }))
    let library = {
      root, name: 'layout-fixture', count: rows.length, active: true, indexing: false, skippedDirs: [],
      metadataIndexing: true, metadataStatus: 'indexing', metadataIndexed: 12345, metadataTotal: 271350,
      metadataFailed: 0, metadataDeferred: 0,
    }
    window.finishLayoutFixture = () => { library = { ...library, metadataIndexing: false, metadataStatus: 'done' } }
    const values = {
      get_status: { library, audioAvailable: true, backend: 'WASAPI', deviceName: 'A long output device name '.repeat(3), sampleRate: 48000, channels: 2, sampleFormat: 'F32' },
      get_sidebar: { facets: [], ucsFacets: [], favorites: 0, total: rows.length },
      list_folders: { path: root, name: 'layout-fixture', count: rows.length, children: [] },
      list_ucs_categories: [], get_history: {}, count_matching: rows.length, search: rows,
      get_waveform: { peaks: [0, 1, 0], lanes: [], durationS: 3 }, get_loop: { enabled: false, region: null },
      get_playback_position: { positionS: 0, durationS: 0, playing: false, paused: false, errorSeq: 0 },
      'plugin:app|version': '0.3.0',
    }
    window.__TAURI_INTERNALS__ = { invoke: async (command) => command === 'list_libraries' ? [library] : values[command] }
  })
  await page.goto(process.argv[2] ?? 'http://127.0.0.1:1420')
  await page.waitForSelector('.metadata-progress')
  const rows = page.locator('.row[data-path]')
  await rows.last().waitFor({ timeout: 5000 })
  const problems = []
  for (const width of [1280, 900, 720, 600]) {
    await page.setViewportSize({ width, height: 650 })
    const geometry = () => page.evaluate(async () => {
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
      const rect = (selector) => document.querySelector(selector).getBoundingClientRect()
      const footer = document.querySelector('.metadata-progress')
      const bar = footer.querySelector('progress') ?? footer.querySelector('ix-progress-indicator')?.shadowRoot?.querySelector('.progress')
      const track = bar?.getBoundingClientRect()
      const label = footer.querySelector('span').getBoundingClientRect()
      const middle = (r) => (r.top + r.bottom) / 2
      return {
        mainRight: rect('.main').right,
        headerRight: rect('.head-cell:last-child').right,
        rowRight: rect('.row[data-path] .cell:last-child').right,
        dockRight: rect('#transport-stop').right,
        versionRight: rect('.statusbar > :last-child').right,
        statusHeight: rect('.statusbar').height,
        tracks: getComputedStyle(document.querySelector('.table-head')).gridTemplateColumns,
        rowTracks: getComputedStyle(document.querySelector('.row[data-path]')).gridTemplateColumns,
        offset: track ? Math.abs(middle(track) - middle(label)) : Infinity,
        barWidth: track?.width ?? 0,
      }
    })
    await rows.first().locator('.cell.name').click()
    const short = await geometry()
    await rows.last().locator('.cell.name').click()
    await page.waitForSelector('.dock-description')
    const long = await geometry()
    // A wide table is allowed to overflow its column now: the tracks are fixed
    // pixels so a dragged width means the same thing at every window size, and the
    // price is a horizontal scrollbar (Explorer does the same). What must hold is
    // that the header and the rows are clipped together, and that the controls
    // outside the scrolling body stay inside the window.
    const body = await page.evaluate(() => {
      const el = document.querySelector('.table-body')
      const main = document.querySelector('.main')
      return {
        scrollWidth: el?.scrollWidth ?? 0,
        clientWidth: el?.clientWidth ?? 0,
        mainWidth: main?.clientWidth ?? 0
      }
    })
    if (body.scrollWidth < body.mainWidth) {
      problems.push(`${width}px: table is narrower than its own container (${body.scrollWidth} < ${body.mainWidth})`)
    }
    for (const [key, value] of Object.entries(long)) {
      if (key === 'dockRight' || key === 'versionRight') {
        if (value > width + 1) problems.push(`${width}px: ${key}=${value}`)
      }
    }
    if (Math.abs(long.headerRight - long.rowRight) > 9) {
      problems.push(
        `${width}px: header right ${long.headerRight} vs row right ${long.rowRight}`
      )
    }
    if (short.tracks !== long.tracks) problems.push(`${width}px: tracks changed ${short.tracks} → ${long.tracks}`)
    if (long.tracks !== long.rowTracks) problems.push(`${width}px: header and row tracks differ`)
    if (long.offset > 1 || long.barWidth < 40) problems.push(`${width}px: progress center offset=${long.offset}, width=${long.barWidth}`)
    assert.equal(long.statusHeight, 26)
    console.log(`${width}px: main right ${long.mainRight}, stop right ${long.dockRight}, progress center offset ${long.offset}px`)
  }
  await page.evaluate(() => window.finishLayoutFixture())
  await page.locator('.metadata-progress').waitFor({ state: 'detached' })
  assert.equal(await page.locator('.statusbar').evaluate((el) => el.getBoundingClientRect().height), 26)
  assert.deepEqual(problems, [])
} finally {
  await browser.close()
}
