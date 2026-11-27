import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
const root = process.argv[3]
if (!root) throw new Error('usage: node tools/check-indexing-acceptance.mjs <port> <library>')

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
if (!page) throw new Error('no page on the debugger')

await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.statusbar', { timeout: 30_000 })
await page.waitForSelector('.row[data-path]', { timeout: 30_000 })
await page.waitForSelector('.row[data-path]', { timeout: 30_000 })

const channelMessages = []
const callbackId = await page.evaluate(() =>
  window.__TAURI_INTERNALS__.transformCallback((raw) => {
    if (!('end' in raw)) window.__indexAcceptanceMessages.push(raw.message)
  })
)
await page.evaluate(() => {
  window.__indexAcceptanceMessages = []
})

const started = Date.now()
const command = page.evaluate(
  ({ library, callback }) =>
    window.__TAURI_INTERNALS__.invoke('add_library', {
      root: library,
      full: true,
      onProgress: `__CHANNEL__:${callback}`
    }),
  { library: root, callback: callbackId }
)

const samples = []
let resolved = false
let commandStatus
command.then((value) => {
  commandStatus = value
  resolved = true
})

while (!resolved) {
  samples.push(await page.evaluate(() => ({
    t: Date.now(),
    overlay: Boolean(document.querySelector('.index-overlay')),
    phase: document.querySelector('.index-phase')?.textContent?.trim() ?? null,
    rows: document.querySelectorAll('.row[data-path]').length
  })))
  await new Promise((resolve) => setTimeout(resolve, 50))
}

const messagesBeforeReload = await page.evaluate(() => window.__indexAcceptanceMessages ?? [])
await page.reload()
await page.waitForSelector('.statusbar', { timeout: 30_000 })

const metadataSamples = []
const interactions = []
const metadataDeadline = Date.now() + 15_000
while (Date.now() < metadataDeadline) {
  const sample = await page.evaluate(async () => {
    const libraries = await window.__TAURI_INTERNALS__.invoke('list_libraries')
    const active = libraries.find((library) => library.active) ?? null
    const bar = document.querySelector('.statusbar')
    const progress = bar?.querySelector('.metadata-progress')
    const version = [...(bar?.children ?? [])].find((element) => /^v\d/.test(element.textContent?.trim() ?? ''))
    const progressRect = progress?.getBoundingClientRect()
    const versionRect = version?.getBoundingClientRect()
    return {
      active: active
        ? {
            indexing: active.metadataIndexing,
            indexed: active.metadataIndexed,
            total: active.metadataTotal,
            failed: active.metadataFailed,
            state: active.metadataStatus
          }
        : null,
      overlay: Boolean(document.querySelector('.index-overlay')),
      rows: document.querySelectorAll('.row[data-path]').length,
      hasProgress: Boolean(progress),
      progressText: progress?.textContent?.trim() ?? null,
      progressBeforeVersion: Boolean(progressRect && versionRect && progressRect.right <= versionRect.left + 1),
      progressHeight: progressRect?.height ?? 0,
      statusbarHeight: bar?.getBoundingClientRect().height ?? 0,
      progressTrackHeight: progress?.querySelector('progress')?.getBoundingClientRect().height ?? 0,
      progressComputedHeight: progress ? getComputedStyle(progress).height : null,
      progressComputedOverflow: progress ? getComputedStyle(progress).overflow : null
    }
  })
  metadataSamples.push({ t: Date.now(), ...sample })

  if (sample.rows > 0 && interactions.length === 0) {
    interactions.push(
      await page.evaluate(async () => {
        const row = document.querySelector('.row[data-path]')
        const path = row?.getAttribute('data-path')
        row?.click()
        if (!path) return { rowClickable: false }
        try {
          await window.__TAURI_INTERNALS__.invoke('count_matching', {
            request: { query: '', limit: 1, offset: 0 }
          })
          await window.__TAURI_INTERNALS__.invoke('set_favorite', { paths: [path], favorite: true })
          await window.__TAURI_INTERNALS__.invoke('set_favorite', { paths: [path], favorite: false })
          await window.__TAURI_INTERNALS__.invoke('get_playback_position')
          return { rowClickable: true, path, query: true, edit: true, playback: true }
        } catch (error) {
          return { rowClickable: true, path, error: String(error) }
        }
      })
    )
  }

  if (sample.active && !sample.active.metadataIndexing && metadataSamples.length > 2) break
  await new Promise((resolve) => setTimeout(resolve, 100))
}

const beforeRemove = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_libraries'))
await page.evaluate((library) => window.__TAURI_INTERNALS__.invoke('remove_library', { root: library }), root)

console.log(
  JSON.stringify(
    {
      commandMs: Date.now() - started,
      commandStatus,
      phases: [...new Set(messagesBeforeReload.map((message) => message.phase))],
      channelMessages: messagesBeforeReload.length,
      samples: samples.slice(0, 12),
      metadataSamples,
      interactions,
      librariesBeforeRemove: beforeRemove.map((library) => ({
        root: library.root,
        active: library.active,
        metadata: library.metadataStatus
      }))
    },
    null,
    2
  )
)

await browser.close()
