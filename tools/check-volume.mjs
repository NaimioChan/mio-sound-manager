// Verify the volume control end to end: the slider moves the renderer signal, the
// IPC command reaches the engine, and the engine reports the value back.
//
//   node tools/check-volume.mjs [port]
//
// The engine's own copy is read through `get_gain`, which is a different command
// from the one the slider writes: a slider that only updated its own state would
// pass a DOM-only check and fail this one.
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
await page.evaluate(() => localStorage.setItem('mio:locale', 'en'))
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(800)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

/** Ask the engine what gain it holds, through the IPC bridge Tauri installs. */
const engineGain = () =>
  page.evaluate(async () => {
    const internals = window.__TAURI_INTERNALS__
    return internals.invoke('get_gain')
  })

/** Set the slider the way a user does: through the iX element's own event. */
const moveSlider = (value) =>
  page.evaluate((value) => {
    const slider = document.querySelector('#volume-slider')
    if (!slider) return 'missing'
    // iX emits `valueChange` with the new number; assigning `.value` alone does not
    // fire it, which is why this dispatches the event the component itself emits.
    slider.value = value
    slider.dispatchEvent(new CustomEvent('valueChange', { detail: value, bubbles: true }))
    return 'ok'
  }, value)

const readout = () => page.evaluate(() => document.querySelector('.volume-value')?.textContent?.trim())

// --- starts at unity or the stored value, and the engine agrees
const startReadout = await readout()
const startEngine = await engineGain()
check(/^\d+%$/.test(startReadout ?? ''), 'the dock shows a percentage', startReadout)
check(
  Math.abs(startEngine - Number((startReadout ?? '100%').replace('%', '')) / 100) < 0.02,
  'the engine gain matches the readout',
  `engine ${startEngine} vs readout ${startReadout}`
)

// --- move it and confirm both sides followed
for (const value of [35, 0, 100, 72]) {
  const moved = await moveSlider(value)
  if (moved !== 'ok') {
    check(false, 'the volume slider exists')
    break
  }
  await page.waitForTimeout(180)
  const shown = await readout()
  const engine = await engineGain()
  check(shown === `${value}%`, `slider ${value} updates the readout`, shown)
  check(
    Math.abs(engine - value / 100) < 0.02,
    `slider ${value} reaches the engine`,
    `engine ${engine}`
  )
}

// --- the mute button flips between 0 and full
await page.click('#volume-mute')
await page.waitForTimeout(200)
const muted = await readout()
const mutedEngine = await engineGain()
check(muted === '0%', 'mute drops the level to 0%', muted)
check(Math.abs(mutedEngine) < 0.001, 'mute reaches the engine', `engine ${mutedEngine}`)

await page.click('#volume-mute')
await page.waitForTimeout(200)
const unmuted = await readout()
const unmutedEngine = await engineGain()
check(unmuted === '100%', 'unmute restores full level', unmuted)
check(Math.abs(unmutedEngine - 1) < 0.001, 'unmute reaches the engine', `engine ${unmutedEngine}`)

// --- the level survives a reload
await moveSlider(40)
await page.waitForTimeout(250)
await page.reload()
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(900)
const afterReload = await readout()
const reloadEngine = await engineGain()
check(afterReload === '40%', 'the level survives a reload', afterReload)
check(
  Math.abs(reloadEngine - 0.4) < 0.02,
  'the reloaded level is pushed back into the engine',
  `engine ${reloadEngine}`
)

// --- leave it at unity for the next check
await moveSlider(100)
await page.waitForTimeout(150)

if (problems.length === 0) {
  console.log('\nvolume: OK')
} else {
  console.log('\nvolume problems:')
  for (const p of problems) console.log(`  - ${p}`)
}
await browser.close()
process.exit(problems.length === 0 ? 0 : 1)
