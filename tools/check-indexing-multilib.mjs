import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
const firstRoot = process.argv[3]
const secondRoot = process.argv[4]
if (!firstRoot || !secondRoot) {
  throw new Error('usage: node tools/check-indexing-multilib.mjs <port> <first-library> <second-library>')
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
if (!page) throw new Error('no page on the debugger')

await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.statusbar', { timeout: 30_000 })

const invoke = (command, args) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
  { command, args }
)

const add = (root) => page.evaluate(async (library) => {
  const messages = []
  const callback = window.__TAURI_INTERNALS__.transformCallback((raw) => {
    if (!('end' in raw)) messages.push(raw.message)
  })
  const result = await window.__TAURI_INTERNALS__.invoke('add_library', {
    root: library,
    full: true,
    onProgress: `__CHANNEL__:${callback}`
  })
  return { result, phases: [...new Set(messages.map((message) => message.phase))] }
}, root)

const first = await add(firstRoot)
await page.reload()
await page.waitForSelector('.row[data-path]', { timeout: 30_000 })
const firstDuring = await invoke('list_libraries')

const second = await add(secondRoot)
await page.reload()
await page.waitForSelector('.row[data-path]', { timeout: 30_000 })
const secondDuring = await invoke('list_libraries')

const activeSnapshot = await page.evaluate(async () => {
  const libraries = await window.__TAURI_INTERNALS__.invoke('list_libraries')
  const active = libraries.find((library) => library.active)
  const bar = document.querySelector('.statusbar')
  const progress = bar?.querySelector('.metadata-progress')
  return {
    activeRoot: active?.root ?? null,
    activeMetadataTotal: active?.metadataTotal ?? null,
    progressText: progress?.textContent?.trim() ?? null,
    progressPresent: Boolean(progress),
    statusbarHeight: bar?.getBoundingClientRect().height ?? null
  }
})

const completion = []
const deadline = Date.now() + 30_000
while (Date.now() < deadline) {
  const libraries = await invoke('list_libraries')
  completion.push(libraries.map((library) => ({
    root: library.root,
    active: library.active,
    indexing: library.metadataIndexing,
    indexed: library.metadataIndexed,
    total: library.metadataTotal,
    status: library.metadataStatus
  })))
  if (libraries.length >= 2 && libraries.every((library) => !library.metadataIndexing)) break
  await new Promise((resolve) => setTimeout(resolve, 100))
}

await page.reload()
await page.waitForSelector('.statusbar', { timeout: 30_000 })
const after = await page.evaluate(() => {
  const bar = document.querySelector('.statusbar')
  return {
    progressPresent: Boolean(bar?.querySelector('.metadata-progress')),
    statusbarHeight: bar?.getBoundingClientRect().height ?? null
  }
})

for (const root of [firstRoot, secondRoot]) {
  await invoke('remove_library', { root })
}

console.log(JSON.stringify({ first, second, firstDuring, secondDuring, activeSnapshot, completion, after }, null, 2))
await browser.close()
