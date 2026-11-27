// node tools/check-favorite.mjs [port]
// The script edits only the temporary library it creates.
import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.argv[2] ?? '9223'}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
const invoke = (command, args) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
  { command, args }
)
const wait = (predicate, args) => page.waitForFunction(predicate, args, { timeout: 20_000 })
await page.waitForSelector('.statusbar', { timeout: 60_000 })
const table = page.locator('.table')
const epoch = () => table.getAttribute('data-epoch')
const rows = page.locator('.row[data-path]')
const star = (index) => rows.nth(index).locator('.cell.favorite')
const settingsKeys = ['mio:locale', 'mio:sidebar-mode', 'mio:settings']
const preferenceSnapshot = await page.evaluate((keys) => keys.map((key) => localStorage.getItem(key)), settingsKeys)
const originalRoot = (await invoke('list_libraries')).find((library) => library.active)?.root
const root = await mkdtemp(join(tmpdir(), 'mio-favorite-'))
const size = 520

const favorites = (count) => wait((expected) => {
  const row = [...document.querySelectorAll('.facet')].find((el) => el.textContent.trim().startsWith('★'))
  const value = row?.querySelector('.count')?.textContent
  return value != null && Number(value.replace(/[^\d]/g, '')) === expected
}, count)
const waitForEpoch = (previous) => wait((was) => {
  const table = document.querySelector('.table')
  return table && table.dataset.epoch !== was && table.dataset.querying === 'false' &&
    table.dataset.epoch === table.dataset.segmentsEpoch
}, previous)
const expectStars = (count, starred) => wait(({ count, starred }) => {
  const cells = [...document.querySelectorAll('.row[data-path] .cell.favorite')].slice(0, count)
  return cells.length === count && cells.every((cell) => cell.classList.contains('on') === starred)
}, { count, starred })

try {
  // Mono, 16-bit PCM, 32 samples. A second segment makes window invalidation observable.
  const wav = Buffer.alloc(108)
  Buffer.from('RIFF').copy(wav)
  wav.writeUInt32LE(wav.length - 8, 4)
  Buffer.from('WAVEfmt ').copy(wav, 8)
  wav.writeUInt32LE(16, 16)
  wav.writeUInt16LE(1, 20)
  wav.writeUInt16LE(1, 22)
  wav.writeUInt32LE(44100, 24)
  wav.writeUInt32LE(88200, 28)
  wav.writeUInt16LE(2, 32)
  wav.writeUInt16LE(16, 34)
  Buffer.from('data').copy(wav, 36)
  wav.writeUInt32LE(wav.length - 44, 40)
  await Promise.all(Array.from({ length: size }, (_, i) =>
    writeFile(join(root, `sound_${String(i).padStart(4, '0')}.wav`), wav)))
  await page.evaluate(async (root) => {
    const ipc = window.__TAURI_INTERNALS__
    const callback = ipc.transformCallback(() => {})
    await ipc.invoke('add_library', { root, onProgress: `__CHANNEL__:${callback}` })
    localStorage.setItem('mio:locale', 'en')
    localStorage.setItem('mio:sidebar-mode', 'categories')
    const settings = JSON.parse(localStorage.getItem('mio:settings') ?? '{}')
    localStorage.setItem('mio:settings', JSON.stringify({ ...settings, autoPlay: false }))
  }, root)
  await page.reload()
  // Check the real path before clicking any edit control.
  await wait((root) => document.querySelector('.row[data-path]')?.dataset.path.startsWith(`${root}\\`), root)
  await wait(async () => {
    const active = (await window.__TAURI_INTERNALS__.invoke('list_libraries')).find((library) => library.active)
    return active && !active.indexing && !active.metadataIndexing
  })
  await favorites(0)
  assert.equal(await invoke('count_matching', { request: {} }), size)

  const firstEpoch = await epoch()
  const started = performance.now()
  await star(0).click()
  await expectStars(1, true)
  console.log(`Click to star: ${Math.round(performance.now() - started)} ms`)
  await favorites(1)
  assert.equal(await epoch(), firstEpoch)
  assert.equal(await table.getAttribute('data-querying'), 'false')
  assert.equal(await page.locator('.row.skeleton').count(), 0)

  const favoritesFilter = page.locator('.facet').filter({ hasText: '★ Favorites' })
  await favoritesFilter.click()
  await waitForEpoch(firstEpoch)
  assert.equal(await rows.count(), 1)
  const filteredEpoch = await epoch()
  await star(0).click()
  await waitForEpoch(filteredEpoch)
  await favorites(0)
  assert.equal(await rows.count(), 0)
  const emptyEpoch = await epoch()
  await favoritesFilter.click()
  await waitForEpoch(emptyEpoch)

  const pathsEpoch = await epoch()
  await rows.nth(0).click()
  await rows.nth(1).click({ modifiers: ['Control'] })
  await rows.nth(2).click({ modifiers: ['Control'] })
  await rows.nth(2).click({ button: 'right' })
  await page.locator('[data-menu-id="favorite"]').click()
  await expectStars(3, true)
  await favorites(3)
  assert.equal(await epoch(), pathsEpoch, 'known paths keep their cached positions')
  await page.locator('#undo-button').click()
  await waitForEpoch(pathsEpoch)
  await favorites(0)
  await expectStars(3, false)

  const matchingEpoch = await epoch()
  await page.keyboard.press('Control+a')
  await rows.nth(0).click({ button: 'right' })
  await page.locator('[data-menu-id="favorite"]').click()
  await waitForEpoch(matchingEpoch)
  await favorites(size)
  await page.keyboard.press('End')
  await wait(() => Boolean(document.querySelector('.row[data-index="519"] .cell.favorite.on')))
  const undoEpoch = await epoch()
  await page.locator('#undo-button').click()
  await waitForEpoch(undoEpoch)
  await favorites(0)
  await wait(() => Boolean(document.querySelector('.row[data-index="519"] .cell.favorite:not(.on)')))
  assert.equal((await invoke('get_summary')).favorites, 0)
  console.log('Favorites: single click, filter, explicit selection, matching selection and undo passed')
} finally {
  try {
    const libraries = await invoke('list_libraries')
    if (libraries.some((library) => library.root === root)) await invoke('remove_library', { root })
    if (originalRoot) await invoke('set_active_library', { root: originalRoot })
  } finally {
    try {
      await page.evaluate(({ keys, values }) => keys.forEach((key, i) => {
        if (values[i] === null) localStorage.removeItem(key)
        else localStorage.setItem(key, values[i])
      }), { keys: settingsKeys, values: preferenceSnapshot })
      await page.reload()
    } finally {
      await browser.close()
      await rm(root, { recursive: true, force: true })
    }
  }
}
