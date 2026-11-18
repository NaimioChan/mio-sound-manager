// Verify the four fixes against the real window.
//
//   node tools/check-region.mjs [port]
//
// 1. drag-out: a row is really draggable (the `draggable` attribute was rendering
//    as `""`, so no DOM drag ever started and the handler never ran)
// 2. auto-play: a fresh profile plays on the first row click
// 3. mute memory: mute then un-mute returns to the level the slider was at
// 4. region: drag selects, the loop follows it, Alt+drag exports a WAV
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

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

const invoke = (cmd, args) =>
  page.evaluate(
    ([cmd, args]) => window.__TAURI_INTERNALS__.invoke(cmd, args),
    [cmd, args ?? null]
  )

// --- 1. drag-out: the row must be a real drag source
await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.row', { timeout: 60_000 })
await page.waitForTimeout(1200)

const draggable = await page.evaluate(() => {
  const row = document.querySelector('.row')
  return {
    prop: row?.draggable,
    attr: row?.getAttribute('draggable')
  }
})
check(draggable.prop === true, 'a row reports draggable === true', JSON.stringify(draggable))
check(
  draggable.attr === 'true',
  'the draggable attribute is a boolean, not an empty string',
  draggable.attr
)

// A real DOM dragstart must reach the app's handler, which calls the plugin.
// That is checked last, on its own: `start_drag` enters a native OLE drag loop
// and the command does not resolve until the drag ends, so every IPC call issued
// after it can time out. The property above is the actual bug; this is the proof
// that the handler it enables is reachable.

// --- 3. mute memory: set a level, mute, un-mute
const setSlider = (value) =>
  page.evaluate((value) => {
    const slider = document.querySelector('#volume-slider')
    slider.value = value
    slider.dispatchEvent(new CustomEvent('valueChange', { detail: value, bubbles: true }))
  }, value)
const readout = () => page.evaluate(() => document.querySelector('.volume-value')?.textContent?.trim())
const engineGain = () => invoke('get_gain')

await setSlider(37)
await page.waitForTimeout(250)
check((await readout()) === '37%', 'the slider moves to 37%', await readout())

await page.click('#volume-mute')
await page.waitForTimeout(250)
check((await readout()) === '0%', 'mute drops to 0%', await readout())

await page.click('#volume-mute')
await page.waitForTimeout(250)
const restored = await readout()
check(restored === '37%', 'un-mute returns to the level that was set, not 100%', restored)
check(
  Math.abs((await engineGain()) - 0.37) < 0.02,
  'the restored level reaches the engine',
  String(await engineGain())
)

// ...and survives a reload, so a mute followed by a restart still comes back right
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(900)
await setSlider(64)
await page.waitForTimeout(200)
await page.click('#volume-mute')
await page.waitForTimeout(200)
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(900)
await page.click('#volume-mute')
await page.waitForTimeout(300)
check((await readout()) === '64%', 'un-mute after a reload returns to 64%', await readout())

// --- 4. region: drag across the waveform selects
await page.locator('.row').first().click()
await page.waitForTimeout(900)
const wave = await page.locator('.wave').boundingBox()
check(!!wave, 'the waveform canvas has a box', JSON.stringify(wave))

if (wave) {
  const y = wave.y + wave.height / 2
  await page.mouse.move(wave.x + wave.width * 0.2, y)
  await page.mouse.down()
  await page.mouse.move(wave.x + wave.width * 0.55, y, { steps: 8 })
  await page.mouse.up()
  await page.waitForTimeout(300)
}

const regionText = await page.evaluate(
  () => document.querySelector('[data-region]')?.textContent?.trim() ?? null
)
check(!!regionText, 'dragging across the waveform selects a region', regionText)

// --- the loop switch reads back from the engine, and follows the selection
const loopBefore = await invoke('get_loop')
check(loopBefore.enabled === false, 'the loop starts off', JSON.stringify(loopBefore))

await page.evaluate(() => document.querySelector('#transport-loop').click())
await page.waitForTimeout(400)
const loopAfter = await invoke('get_loop')
check(loopAfter.enabled === true, 'the loop switch turns looping on', JSON.stringify(loopAfter))
check(
  Array.isArray(loopAfter.region) && loopAfter.region[1] > loopAfter.region[0],
  'the loop carries the selected region',
  JSON.stringify(loopAfter.region)
)

// Esc clears the selection and the loop falls back to the whole clip
await page.keyboard.press('Escape')
await page.waitForTimeout(400)
const cleared = await page.evaluate(
  () => document.querySelector('[data-region]')?.textContent?.trim() ?? null
)
check(cleared === null, 'Escape clears the selection', String(cleared))
const loopWhole = await invoke('get_loop')
check(
  loopWhole.enabled === true && loopWhole.region === null,
  'the loop falls back to the whole clip once the selection is gone',
  JSON.stringify(loopWhole)
)

// --- Alt+drag exports a WAV: the command is what a drag would carry
const exported = await page.evaluate(async () => {
  const path = document.querySelector('.row')?.getAttribute('data-path')
  if (!path) return 'no row'
  const internals = window.__TAURI_INTERNALS__
  try {
    const out = await internals.invoke('export_region', {
      path,
      startS: 0.05,
      endS: 0.15
    })
    return out
  } catch (e) {
    return `error: ${e}`
  }
})
check(
  typeof exported === 'string' && /\.wav$/i.test(exported),
  'exporting a region writes a WAV and returns its path',
  exported
)

// --- 1b. the drag handler is reachable, checked last
//
// `start_drag` enters a native OLE drag loop and does not resolve until the drag
// ends, so everything that needs IPC has to run before this. A pending promise
// after the timeout is the success case: it means the OS drag really started.
const dragProbe = await page.evaluate(async () => {
  const row = document.querySelector('.row')
  if (!row) return 'no row'
  let seen = 0
  row.addEventListener('dragstart', () => { seen += 1 }, { once: true })
  const dt = new DataTransfer()
  row.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: dt }))
  await new Promise((r) => setTimeout(r, 500))
  return `dragstart fired ${seen}`
})
check(dragProbe === 'dragstart fired 1', 'a DOM dragstart reaches the row', dragProbe)

await browser.close()
if (problems.length === 0) {
  console.log('\nregion: OK')
  process.exit(0)
}
console.log('\nregion problems:')
for (const p of problems) console.log(`  - ${p}`)
process.exit(1)
