// Layout sanity: catch overlap and clipping by measuring the real DOM, which is
// more reliable than looking at a screenshot. Run against the live window.
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

const report = await page.evaluate(() => {
  const out = { problems: [], facts: {} }

  /**
   * Does this element's text overflow the box it was given?
   *
   * `text-overflow: ellipsis` makes overflow *intentional*: a long filename in a
   * narrow column is supposed to end in `…`, and reporting it would mean the check
   * only passes at one particular rail width. What this is looking for is text cut
   * off with no ellipsis, which is the shape a column-track mismatch takes.
   */
  const clipped = (el, label) => {
    const overflows = el.scrollWidth > el.clientWidth + 1
    if (overflows && getComputedStyle(el).textOverflow !== 'ellipsis') {
      out.problems.push(`${label}: text clipped (${el.scrollWidth} > ${el.clientWidth})`)
    }
    if (el.scrollHeight > el.clientHeight + 1 && el.clientHeight > 0) {
      out.problems.push(`${label}: content clipped vertically (${el.scrollHeight} > ${el.clientHeight})`)
    }
  }

  /** Do two elements' boxes intersect? */
  const overlaps = (a, b, label) => {
    const ra = a.getBoundingClientRect()
    const rb = b.getBoundingClientRect()
    const x = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left)
    const y = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top)
    if (x > 1 && y > 1) out.problems.push(`${label}: overlap ${x.toFixed(0)}×${y.toFixed(0)}px`)
  }

  // --- structure present
  const regions = {
    topbar: document.querySelector('.topbar'),
    sidebar: document.querySelector('.sidebar'),
    table: document.querySelector('.table'),
    dock: document.querySelector('.dock'),
    statusbar: document.querySelector('.statusbar')
  }
  for (const [name, el] of Object.entries(regions)) {
    if (!el) out.problems.push(`missing region: ${name}`)
  }

  // --- the main regions must tile the window without overlapping
  const order = ['topbar', 'sidebar', 'table', 'dock', 'statusbar']
  out.facts.regions = order.map((name) => {
    const r = regions[name]?.getBoundingClientRect()
    return r
      ? `${name} ${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}×${Math.round(r.height)}`
      : `${name} absent`
  })
  if (regions.topbar && regions.sidebar) {
    const tb = regions.topbar.getBoundingClientRect()
    const sb = regions.sidebar.getBoundingClientRect()
    if (sb.top < tb.bottom - 1) out.problems.push('sidebar starts above the top bar')
  }

  // --- table header must line up with the row grid
  const head = document.querySelector('.table-head')
  const row = document.querySelector('.row')
  if (head && row) {
    const parse = (value) => value.split(/\s+/).map((t) => Number.parseFloat(t))
    const headTracks = parse(getComputedStyle(head).gridTemplateColumns)
    const rowTracks = parse(getComputedStyle(row).gridTemplateColumns)
    if (headTracks.length !== rowTracks.length) {
      out.problems.push(`column count differs: head ${headTracks.length}, row ${rowTracks.length}`)
    } else {
      // Half a pixel of tolerance: the header is `100% - scrollbar` while the row
      // is the body's content box, and the two can round apart by a subpixel.
      // Anything above that is a visible drift.
      const drift = headTracks.map((t, i) => Math.abs(t - rowTracks[i]))
      const worst = Math.max(...drift)
      if (worst > 0.5) {
        out.problems.push(
          `column tracks differ by ${worst.toFixed(2)}px:\n  head ${headTracks.join(' ')}\n  row  ${rowTracks.join(' ')}`
        )
      }
    }
    out.facts.columns = getComputedStyle(head).gridTemplateColumns
  }

  // --- every visible cell must fit its track
  document.querySelectorAll('.row .cell').forEach((cell, i) => {
    clipped(cell, `cell[${i}] ${cell.textContent?.slice(0, 20)}`)
  })

  // --- header labels must fit their buttons
  document.querySelectorAll('.head-cell').forEach((el, i) => {
    clipped(el, `head-cell[${i}]`)
  })

  // --- facets and folder rows
  document.querySelectorAll('.facet, .folder').forEach((el, i) => {
    clipped(el, `rail row[${i}]`)
  })

  // --- the dock's header line must not run into its buttons
  const dockHead = document.querySelector('.dock-head')
  if (dockHead) {
    const kids = [...dockHead.children].filter((k) => k.getBoundingClientRect().width > 0)
    for (let i = 0; i < kids.length - 1; i++) {
      overlaps(kids[i], kids[i + 1], `dock-head child ${i}/${i + 1}`)
    }
    clipped(dockHead, 'dock-head')
  }

  // --- status bar must not clip its own line
  const status = document.querySelector('.statusbar')
  if (status) clipped(status, 'statusbar')

  // --- nothing may spill outside the viewport
  document.querySelectorAll('.topbar > *, .statusbar > *, .dock-head > *').forEach((el, i) => {
    const r = el.getBoundingClientRect()
    if (r.right > window.innerWidth + 1) {
      out.problems.push(`element spills past the right edge: ${el.className} (${Math.round(r.right)} > ${window.innerWidth})`)
    }
    if (r.left < -1) out.problems.push(`element spills past the left edge: ${el.className}`)
  })

  out.facts.viewport = `${window.innerWidth}×${window.innerHeight}`
  out.facts.rowsRendered = document.querySelectorAll('.row').length
  out.facts.bodyScroll = `${document.body.scrollWidth}×${document.body.scrollHeight}`
  return out
})

console.log('viewport:', report.facts.viewport, ' body:', report.facts.bodyScroll)
console.log('rows rendered:', report.facts.rowsRendered)
console.log('regions:')
for (const line of report.facts.regions ?? []) console.log('  ', line)
if (report.facts.columns) console.log('columns:', report.facts.columns)

if (report.problems.length === 0) {
  console.log('\nOK — no overlap, no clipping')
} else {
  console.log(`\n${report.problems.length} problem(s):`)
  for (const p of report.problems) console.log('  -', p)
}

await browser.close()
process.exit(report.problems.length === 0 ? 0 : 1)
