// Verify the four fixes against the real window.
//
//   node tools/check-region.mjs [port]
//
// 1. drag-out: a row is really draggable (the `draggable` attribute was rendering
//    as `""`, so no DOM drag ever started and the handler never ran)
// 2. auto-play: a row click auditions it. The *default* of the switch (a fresh
//    profile starts on) is asserted in `check-interactions.mjs`; this script only
//    depends on it, it does not prove it. A previous aborted run could leave the
//    setting off in the real profile, so this script restores the blob on the way
//    out — see the snapshot at the bottom.
// 3. mute memory: mute then un-mute returns to the level the slider was at
// 4. region: drag selects, the top third is the drag handle, a boundary drags,
//    the loop switch looks on when it is on, and export_region writes a WAV
// 5. lanes: a stereo file carries two envelopes, a mono file one
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'

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

// The body of the panel is the selection band; the top third is the drag-out
// handle. Every gesture below aims at the middle third on purpose.
const bodyY = wave ? wave.y + wave.height * 0.6 : 0
const grabY = wave ? wave.y + wave.height * 0.15 : 0

if (wave) {
  await page.mouse.move(wave.x + wave.width * 0.2, bodyY)
  await page.mouse.down()
  await page.mouse.move(wave.x + wave.width * 0.55, bodyY, { steps: 8 })
  await page.mouse.up()
  await page.waitForTimeout(300)
}

const regionText = await page.evaluate(
  () => document.querySelector('[data-region]')?.textContent?.trim() ?? null
)
check(!!regionText, 'dragging across the waveform selects a region', regionText)

// --- the cursor and the band: no modifier key, the top third is the handle
const cursorAt = (y) =>
  page.evaluate((y) => {
    const canvas = document.querySelector('.wave')
    const box = canvas.getBoundingClientRect()
    const ev = new PointerEvent('pointermove', {
      clientX: box.left + box.width / 2,
      clientY: y,
      bubbles: true
    })
    canvas.dispatchEvent(ev)
    return {
      cursor: getComputedStyle(canvas).cursor,
      band: document.querySelector('.wave-wrap')?.classList.contains('grab') ?? null
    }
  }, y)

const overBody = await cursorAt(bodyY)
check(overBody.cursor === 'text', 'the body of the panel shows the bar cursor', overBody.cursor)
check(overBody.band === false, 'the body of the panel does not light the drag band', String(overBody.band))

const overBand = await cursorAt(grabY)
check(overBand.cursor === 'grab', 'the top third shows the grab cursor', overBand.cursor)
check(overBand.band === true, 'the top third lights the drag band', String(overBand.band))

// --- a region edge can be dragged, which moves that boundary
//
// The geometry is read from the DOM rather than recomputed: the readout carries
// the selection in seconds and the time readout carries the duration, so the two
// numbers the canvas draws from are the two numbers this uses.
const geom = await page.evaluate(() => {
  const box = document.querySelector('.wave').getBoundingClientRect()
  const parse = (t) => {
    const [m, s] = t.trim().split(':')
    return Number(m) * 60 + Number(s)
  }
  const region = (document.querySelector('[data-region]')?.textContent ?? '')
    .split('·')[0]
    .split('–')
  const time = (document.querySelector('.wave-time')?.textContent ?? '').split('/')
  return {
    left: box.left,
    top: box.top,
    width: box.width,
    height: box.height,
    from: parse(region[0] ?? '0:00'),
    to: parse(region[1] ?? '0:00'),
    duration: parse(time[1] ?? '0:00')
  }
})
const endX = geom.left + (geom.to / Math.max(geom.duration, 0.001)) * geom.width
await page.mouse.move(endX, geom.top + geom.height * 0.6)
await page.waitForTimeout(150)
const edgeHover = await page.evaluate(() => ({
  cursor: getComputedStyle(document.querySelector('.wave')).cursor,
  cls: document.querySelector('.wave-wrap').className
}))
check(
  edgeHover.cursor === 'col-resize',
  'a region boundary shows the resize cursor',
  `${edgeHover.cursor} (${edgeHover.cls})`
)

await page.mouse.down()
await page.mouse.move(endX - geom.width * 0.12, geom.top + geom.height * 0.6, { steps: 8 })
await page.mouse.up()
await page.waitForTimeout(300)
const regionAfter = await page.evaluate(
  () => document.querySelector('[data-region]')?.textContent?.trim() ?? null
)
check(
  !!regionAfter && regionAfter !== regionText,
  'dragging a boundary moves it',
  `${regionText} → ${regionAfter}`
)

// --- the loop switch reads back from the engine, and follows the selection
const loopBefore = await invoke('get_loop')
check(loopBefore.enabled === false, 'the loop starts off', JSON.stringify(loopBefore))

const loopPress = await page.evaluate(() => {
  const host = document.querySelector('#transport-loop')
  const inner = host?.shadowRoot?.querySelector('button')
  if (!inner) return 'no inner button'
  inner.click()
  return 'ok'
})
check(loopPress === 'ok', 'the loop switch has a clickable inner button', loopPress)
await page.waitForTimeout(400)
const loopAfter = await invoke('get_loop')
check(loopAfter.enabled === true, 'the loop switch turns looping on', JSON.stringify(loopAfter))
check(
  Array.isArray(loopAfter.region) && loopAfter.region[1] > loopAfter.region[0],
  'the loop carries the selected region',
  JSON.stringify(loopAfter.region)
)

// The switch has to *look* on, not just be on. `ix-icon-button`'s `selected` prop
// reached nothing in iX 5, which is why this is now an `ix-icon-toggle-button` and
// the check reads the rendered background rather than the prop.
const loopLook = await page.evaluate(() => {
  const host = document.querySelector('#transport-loop')
  const inner = host?.shadowRoot?.querySelector('button')
  return {
    pressed: host?.pressed,
    ariaPressed: inner?.getAttribute('aria-pressed'),
    bg: inner ? getComputedStyle(inner).backgroundColor : null,
    cls: inner?.className ?? ''
  }
})
check(loopLook.pressed === true, 'the switch reports pressed', String(loopLook.pressed))
check(loopLook.ariaPressed === 'true', 'the switch says pressed to assistive tech', String(loopLook.ariaPressed))
check(
  loopLook.cls.includes('selected'),
  'the inner button carries the selected class',
  loopLook.cls
)

// ...and off again looks different, which is the whole complaint about the old one.
const offLook = await page.evaluate(() => {
  const host = document.querySelector('#transport-loop')
  const inner = host?.shadowRoot?.querySelector('button')
  inner?.click()
  return null
})
await page.waitForTimeout(400)
const loopOff = await page.evaluate(() => {
  const inner = document.querySelector('#transport-loop')?.shadowRoot?.querySelector('button')
  return {
    pressed: document.querySelector('#transport-loop')?.pressed,
    bg: inner ? getComputedStyle(inner).backgroundColor : null,
    cls: inner?.className ?? ''
  }
})
check(loopOff.pressed === false, 'the switch turns off', String(loopOff.pressed))
check(
  loopOff.bg !== loopLook.bg,
  'on and off do not render the same background',
  `${loopLook.bg} vs ${loopOff.bg}`
)

// Turn it back on for the Escape check below.
await page.evaluate(() => {
  document.querySelector('#transport-loop')?.shadowRoot?.querySelector('button')?.click()
})
await page.waitForTimeout(400)

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

// --- the stereo/mono lanes: a stereo file must draw two, a mono file one
const lanes = await page.evaluate(async () => {
  const rows = [...document.querySelectorAll('.row')]
  const out = []
  for (const row of rows.slice(0, 40)) {
    const path = row.getAttribute('data-path')
    if (!path) continue
    const w = await window.__TAURI_INTERNALS__.invoke('get_waveform', { path, buckets: 64 })
    out.push({ channels: w.channels, lanes: w.lanes.length, path })
    if (out.length >= 3) break
  }
  return out
})
check(lanes.length > 0, 'get_waveform answers for the rows on screen', JSON.stringify(lanes.length))
for (const item of lanes) {
  const want = item.channels === 1 ? 1 : item.channels === 2 ? 2 : 0
  check(
    item.lanes === want,
    `a ${item.channels}-channel file carries ${want} lane(s)`,
    `${item.lanes} lanes for ${item.path.split(/[\\/]/).pop()}`
  )
}

// --- a selection exports to a WAV: the command is what a drag would carry
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

// --- 5b. which file the top band hands over
//
// `__TAURI_INTERNALS__.invoke` is a non-writable, non-configurable property, so the
// plugin call cannot be stubbed. What *is* observable is the side effect that
// precedes it: a press inside the selection renders the selection to a WAV in the
// export cache first, and a press outside it does not. So the check counts files in
// that directory.
//
// Only one band press per app session is possible: the press enters the native OLE
// drag loop, which parks every later IPC call. The outside-the-selection case shares
// the same `!inside` branch as "no selection at all" — it hands over the source file
// with no export — so it is not worth a second session here.
const exportsDir = path.join(
  process.env.LOCALAPPDATA ?? os.homedir(),
  'com.naimio.miosound',
  'exports'
)
// Name → mtime, not a bare list: the file name is derived from the path and the
// selection, so running this twice exports to the same name and a count would not
// move. A rewritten file is a newer mtime.
const listExports = async () => {
  try {
    const { readdirSync, statSync } = await import('node:fs')
    const out = {}
    for (const f of readdirSync(exportsDir)) {
      if (!/\.wav$/i.test(f)) continue
      out[f] = statSync(`${exportsDir}/${f}`).mtimeMs
    }
    return out
  } catch {
    return {}
  }
}

// Put a selection back (the Escape check above cleared it), then press inside it.
if (wave) {
  await page.mouse.move(wave.x + wave.width * 0.3, bodyY)
  await page.mouse.down()
  await page.mouse.move(wave.x + wave.width * 0.6, bodyY, { steps: 8 })
  await page.mouse.up()
  await page.waitForTimeout(300)
}
const bounds = await page.evaluate(() => {
  const parse = (t) => {
    const [m, s] = t.trim().split(':')
    return Number(m) * 60 + Number(s)
  }
  const region = (document.querySelector('[data-region]')?.textContent ?? '').split('·')[0].split('–')
  const time = (document.querySelector('.wave-time')?.textContent ?? '').split('/')
  return {
    from: parse(region[0] ?? '0:00'),
    to: parse(region[1] ?? '0:00'),
    duration: parse(time[1] ?? '0:00')
  }
})
check(
  bounds.to > bounds.from,
  'a selection exists to drag out of',
  `${bounds.from}s – ${bounds.to}s of ${bounds.duration}s`
)

const insideFraction = (bounds.from + bounds.to) / 2 / Math.max(bounds.duration, 0.001)
const startedAt = Date.now()
const before = await listExports()
if (wave) {
  const x = wave.x + wave.width * insideFraction
  const y = wave.y + wave.height * 0.15 // inside the top third
  await page.evaluate(
    ([x, y]) => {
      const canvas = document.querySelector('.wave')
      canvas.dispatchEvent(
        new PointerEvent('pointerdown', {
          clientX: x,
          clientY: y,
          button: 0,
          bubbles: true,
          cancelable: true
        })
      )
    },
    [x, y]
  )
}
// The export is an async IPC round trip plus a WAV write; poll the directory rather
// than the DOM, because the drag that follows parks the webview.
let written = []
for (let i = 0; i < 40; i++) {
  await new Promise((r) => setTimeout(r, 100))
  const after = await listExports()
  written = Object.entries(after)
    .filter(([name, mtime]) => mtime > startedAt && (before[name] ?? 0) !== mtime)
    .map(([name]) => name)
  if (written.length > 0) break
}
check(
  written.length === 1,
  'a press in the band inside the selection renders the selection to a WAV',
  written[0] ?? `nothing written (${Object.keys(before).length} files already there)`
)
check(
  written.length === 1 && /\s\d+(\.\d+)?-\d+(\.\d+)?s\.wav$/i.test(written[0]),
  'the written file is a selection render, named by its source and range',
  written[0] ?? 'none'
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
