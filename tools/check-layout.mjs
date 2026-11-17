// Layout sanity for the rewritten UI: measure the real DOM instead of looking at
// a screenshot.
//
//   node tools/check-layout.mjs [port]
//
// Reports overlaps, clipping without ellipsis, header/row column drift, and the
// dock's control cluster order. Exits non-zero when a problem is found.
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
await page.waitForSelector('.app', { timeout: 20_000 })
await page.waitForTimeout(800)

const report = await page.evaluate(() => {
  const problems = []
  const facts = {}

  /** Text cut off with no ellipsis: the shape a column-track mismatch takes. */
  const clipped = (el, label) => {
    if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).textOverflow !== 'ellipsis') {
      problems.push(`${label}: text clipped (${el.scrollWidth} > ${el.clientWidth})`)
    }
    if (el.scrollHeight > el.clientHeight + 1 && el.clientHeight > 0) {
      problems.push(`${label}: content clipped vertically (${el.scrollHeight} > ${el.clientHeight})`)
    }
  }

  const overlaps = (a, b, label) => {
    const ra = a.getBoundingClientRect()
    const rb = b.getBoundingClientRect()
    const x = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left)
    const y = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top)
    if (x > 1 && y > 1) problems.push(`${label}: overlap ${x.toFixed(0)}×${y.toFixed(0)}px`)
  }

  // --- structure
  const regions = {
    topbar: document.querySelector('.topbar'),
    sidebar: document.querySelector('.sidebar'),
    table: document.querySelector('.table'),
    dock: document.querySelector('.dock'),
    statusbar: document.querySelector('.statusbar')
  }
  for (const [name, el] of Object.entries(regions)) {
    if (!el) problems.push(`missing region: ${name}`)
  }

  // --- the regions tile the window
  facts.regions = Object.entries(regions).map(([name, el]) => {
    const r = el?.getBoundingClientRect()
    return r
      ? `${name} ${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}×${Math.round(r.height)}`
      : `${name} absent`
  })
  if (regions.topbar && regions.sidebar) {
    const tb = regions.topbar.getBoundingClientRect()
    const sb = regions.sidebar.getBoundingClientRect()
    if (sb.top < tb.bottom - 1) problems.push('sidebar starts above the top bar')
  }
  if (regions.table && regions.dock) {
    const t = regions.table.getBoundingClientRect()
    const d = regions.dock.getBoundingClientRect()
    if (Math.abs(t.bottom - d.top) > 1) {
      problems.push(`table/dock gap: table ends ${t.bottom}, dock starts ${d.top}`)
    }
  }

  // --- header tracks must match the row tracks
  const head = document.querySelector('.table-head')
  const row = document.querySelector('.row')
  if (head && row) {
    const parse = (v) => v.split(/\s+/).map((t) => Number.parseFloat(t))
    const ht = parse(getComputedStyle(head).gridTemplateColumns)
    const rt = parse(getComputedStyle(row).gridTemplateColumns)
    if (ht.length !== rt.length) {
      problems.push(`column count differs: head ${ht.length}, row ${rt.length}`)
    } else {
      const worst = Math.max(...ht.map((t, i) => Math.abs(t - rt[i])))
      if (worst > 0.5) {
        problems.push(
          `column tracks differ by ${worst.toFixed(2)}px\n  head ${ht.join(' ')}\n  row  ${rt.join(' ')}`
        )
      }
    }
    facts.columns = getComputedStyle(head).gridTemplateColumns
    // Header right edge vs row right edge: the scrollbar allowance.
    const hr = head.getBoundingClientRect()
    const rr = row.getBoundingClientRect()
    facts.headRight = Math.round(hr.right)
    facts.rowRight = Math.round(rr.right)
    if (Math.abs(hr.right - rr.right) > 9) {
      problems.push(`header right edge off by ${Math.round(hr.right - rr.right)}px`)
    }
  } else {
    problems.push('no table header or row to compare')
  }

  // --- cells must fit
  document.querySelectorAll('.row .cell').forEach((cell, i) => {
    clipped(cell, `cell[${i}] ${cell.textContent?.slice(0, 20)}`)
  })
  document.querySelectorAll('.head-cell').forEach((el, i) => clipped(el, `head-cell[${i}]`))
  document.querySelectorAll('.facet, .folder, .lib-head').forEach((el, i) =>
    clipped(el, `rail row[${i}] ${el.textContent?.slice(0, 24)}`)
  )

  // --- the dock's controls must not overlap, and the order must be
  //     volume → AUTO PLAY → pause → stop
  const dockHead = document.querySelector('.dock-head')
  if (dockHead) {
    const kids = [...dockHead.children].filter((k) => k.getBoundingClientRect().width > 0)
    for (let i = 0; i < kids.length - 1; i++) {
      overlaps(kids[i], kids[i + 1], `dock-head child ${i}/${i + 1}`)
    }
    clipped(dockHead, 'dock-head')
    facts.dockOrder = kids.map((el) => {
      const name = el.id || el.className?.toString().split(' ')[0] || el.tagName.toLowerCase()
      const r = el.getBoundingClientRect()
      return `${name} ${Math.round(r.left)}..${Math.round(r.right)}`
    })

    const volume = dockHead.querySelector('.volume')
    const auto = dockHead.querySelector('#auto-play')
    const toggle = dockHead.querySelector('#transport-toggle')
    const stop = dockHead.querySelector('#transport-stop')
    if (!volume) problems.push('no volume control in the dock')
    if (!auto) problems.push('no AUTO PLAY toggle in the dock')
    if (volume && auto) {
      const v = volume.getBoundingClientRect()
      const a = auto.getBoundingClientRect()
      if (v.right > a.left + 1) problems.push('volume control is not left of AUTO PLAY')
      if (a.left - v.right > 16) problems.push(`volume is ${Math.round(a.left - v.right)}px from AUTO PLAY`)
    }
    if (auto && toggle && stop) {
      const a = auto.getBoundingClientRect()
      const t = toggle.getBoundingClientRect()
      const s = stop.getBoundingClientRect()
      if (!(a.right <= t.left + 1 && t.right <= s.left + 1)) {
        problems.push('transport order is not AUTO PLAY → pause → stop')
      }
    }
    const slider = dockHead.querySelector('#volume-slider')
    if (!slider) problems.push('no volume slider')
    else {
      const r = slider.getBoundingClientRect()
      facts.volumeSlider = `${Math.round(r.width)}×${Math.round(r.height)} at ${Math.round(r.left)}`
      if (r.width < 60 || r.width > 200) problems.push(`volume slider width ${r.width}px is out of range`)
    }
  }

  // --- nothing may overflow the window
  //
  // Elements inside a scrolling container legitimately extend past the viewport:
  // the rail scrolls, and so does the table. What this looks for is content that
  // escapes the window *without* a scroll container to hide it, which is the shape
  // a broken grid or an absolutely positioned panel takes.
  const inScroller = (el) => {
    for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
      const overflow = getComputedStyle(p).overflowY
      if (overflow === 'auto' || overflow === 'scroll' || overflow === 'hidden') return true
    }
    return false
  }
  const overflow = [...document.querySelectorAll('.app *')].filter((el) => {
    const r = el.getBoundingClientRect()
    if (r.width === 0) return false
    const escapes = r.right > window.innerWidth + 1 || r.bottom > window.innerHeight + 1
    return escapes && !inScroller(el)
  })
  if (overflow.length > 0) {
    problems.push(
      `elements overflow the window: ${overflow
        .slice(0, 4)
        .map((el) => `${el.tagName.toLowerCase()}.${el.className?.toString().split(' ')[0]}`)
        .join(', ')}`
    )
  }

  facts.rows = document.querySelectorAll('.row').length
  facts.theme = document.documentElement.dataset.mioTheme ?? 'none'
  return { problems, facts }
})

console.log(JSON.stringify(report.facts, null, 1))
if (report.problems.length === 0) {
  console.log('\nlayout: OK')
} else {
  console.log('\nlayout problems:')
  for (const p of report.problems) console.log(`  - ${p}`)
}
await browser.close()
process.exit(report.problems.length === 0 ? 0 : 1)
