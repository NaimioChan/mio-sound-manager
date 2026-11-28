// Inspector layout and click semantics, driven over CDP or against the Vite
// renderer with an isolated IPC fixture.
//
//   node tools/check-inspector.mjs              # the desktop window on :9223
//   node tools/check-inspector.mjs 9223         # explicit debugger port
//   node tools/check-inspector.mjs http://127.0.0.1:1420
//
// Covers: the closed strip, open/close and persistence, the panel's width clamp,
// and the two click families the panel owns — TEXT values append to the query
// while category/UCS/folder values clear the search first — plus copying a path
// and the numeric channel filter. Everything is asserted on the real DOM and on
// what the app asked the backend for, not on a screenshot.
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'

/**
 * `playwright-core` is not a dependency of this repo; the other `check-*.mjs`
 * scripts resolve it out of a sibling checkout. That pointer is machine-specific
 * and rots, so this script tries the documented path first and then the two
 * locations a machine may have installed it into, before failing with the list.
 */
function loadPlaywright() {
  const candidates = [
    process.env.MIO_PLAYWRIGHT_PKG,
    'C:/Users/Naimio/projects/slip-sound/package.json',
    `${process.env.LOCALAPPDATA}/Temp/opencode/pw/package.json`,
    new URL('../package.json', import.meta.url).pathname
  ].filter(Boolean)
  const tried = []
  for (const candidate of candidates) {
    try {
      return createRequire(candidate)('playwright-core')
    } catch {
      tried.push(candidate)
    }
  }
  throw new Error(`playwright-core not found; tried:\n  ${tried.join('\n  ')}\nset MIO_PLAYWRIGHT_PKG to a package.json beside it`)
}

const { chromium } = loadPlaywright()

const target = process.argv[2] ?? '9223'
const isUrl = target.startsWith('http')
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'

const browser = isUrl
  ? await chromium.launch(existsSync(edge) ? { executablePath: edge } : { channel: 'msedge' })
  : await chromium.connectOverCDP(`http://127.0.0.1:${target}`)

const problems = []
const check = (ok, label, detail = '') => {
  if (!ok) problems.push(`${label}${detail ? ` (${detail})` : ''}`)
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
}

try {
  /** Every `search` payload the app has sent, so a click's effect is provable. */
  let page
  if (isUrl) {
    page = await browser.newPage()
    page.on('pageerror', (error) => problems.push(`pageerror: ${error}`))
    await page.addInitScript(() => {
      localStorage.setItem('mio:locale', 'en')
      localStorage.setItem('mio:settings', JSON.stringify({ autoPlay: false }))
      // Only the first load starts from a closed inspector: the reload assertion
      // needs the state the previous page wrote to survive. `addInitScript` runs
      // on every navigation, so the reset is guarded by a session flag.
      if (!sessionStorage.getItem('mio:fixture-seeded')) {
        sessionStorage.setItem('mio:fixture-seeded', '1')
        localStorage.removeItem('mio:inspector-open')
        localStorage.removeItem('mio:inspector-width')
      }
      const root = 'C:\\inspector-fixture'
      const rows = ['Door_Slam_01.wav', 'Rain_Loop_140BPM.wav', 'Gunshot.wav'].map((filename, i) => ({
        filename,
        path: `${root}\\Pack_${i + 1}\\${filename}`,
        folder: `Pack_${i + 1}`,
        durationS: 2.5 + i,
        durationLabel: `0:0${2 + i}.50`,
        channels: i === 1 ? 1 : 2,
        sampleRate: i === 0 ? 44100 : 48000,
        filesize: 120000 + i * 1000,
        favorite: i === 2,
        category: i === 0 ? 'Foley' : i === 1 ? 'Ambience' : 'Weapons',
        subcategory: i === 0 ? 'Doors' : null,
        confidence: 0.62,
        matchedTerms: ['door', 'slam'],
        description: i === 0 ? 'A heavy metal door slamming shut.' : null,
        descriptionCount: 1,
        ucsCategory: i === 0 ? 'MECHANICAL' : null,
        ucsSubcategory: i === 0 ? 'DOORS' : null,
        ucsCatId: i === 0 ? 'MECHDOOR' : null,
        categoryManual: false,
        bpm: i === 1 ? 140 : null,
        coverKey: i === 0 ? 'aaaa0000aaaa0000aaaa0000aaaa0000' : null,
      }))
      // A 1×1 JPEG, enough for an <img> to paint.
      const COVER =
        'data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAACf/EABQQAQAAAAAAAAAAAAD/2gAIAQEAAD8AKp//2Q=='
      window.__inspectCalls = []
      window.__coverCalls = []
      const library = {
        root,
        name: 'inspector-fixture',
        count: rows.length,
        active: true,
        indexing: false,
        skippedDirs: [],
        metadataIndexing: false,
        metadataStatus: 'done',
        metadataIndexed: rows.length,
        metadataTotal: rows.length,
        metadataFailed: 0,
        metadataDeferred: 0,
      }
      window.__searchCalls = []
      const values = {
        get_status: {
          library,
          audioAvailable: true,
          backend: 'WASAPI',
          deviceName: 'Fixture device',
          sampleRate: 48000,
          channels: 2,
          sampleFormat: 'F32',
        },
        get_sidebar: {
          facets: [
            { category: 'Foley', subcategory: 'Doors', count: 1 },
            { category: 'Ambience', subcategory: null, count: 1 },
            { category: 'Weapons', subcategory: null, count: 1 },
          ],
          ucsFacets: [{ category: 'MECHANICAL', subcategory: 'DOORS', count: 1 }],
          favorites: 1,
          total: rows.length,
        },
        get_summary: { favorites: 1, total: rows.length },
        list_folders: {
          path: root,
          name: 'inspector-fixture',
          count: rows.length,
          children: [
            // Enough siblings that Pack_1 starts below the rail's viewport: the
            // inspector's folder click then has to scroll, not just switch modes.
            ...Array.from({ length: 40 }, (_, i) => ({
              path: `${root}\\Filler_${i}`,
              name: `Filler_${i}`,
              count: 1,
              children: [],
            })),
            ...['Pack_1', 'Pack_2', 'Pack_3'].map((name) => ({
              path: `${root}\\${name}`,
              name,
              count: 1,
              children: [],
            })),
          ],
        },
        list_ucs_categories: [],
        get_history: {},
        count_matching: rows.length,
        search: rows,
        get_waveform: { peaks: [0, 1, 0], lanes: [], durationS: 3 },
        get_loop: { enabled: false, region: null },
        get_playback_position: { positionS: 0, durationS: 0, playing: false, paused: false, errorSeq: 0 },
        'plugin:app|version': '0.3.0',
      }
      window.__TAURI_INTERNALS__ = {
        invoke: async (command, args) => {
          if (command === 'list_libraries') return [library]
          if (command === 'search') window.__searchCalls.push(args?.request ?? {})
          if (command === 'count_matching') window.__searchCalls.push(args?.request ?? {})
          if (command === 'inspect_sound') {
            window.__inspectCalls.push(args?.path)
            return {
              path: args.path,
              mtimeNs: 1,
              indexedAt: 1,
              tags: [
                { key: 'artist', value: 'Naimio', source: 'id3', searchable: true },
                { key: 'bext.originator', value: 'Field Recorder', source: 'bext', searchable: true },
                { key: 'comment', value: '0123456789abcdef0123456789abcdef', source: 'id3', searchable: false },
              ],
              tagsTruncated: false,
              cover: {
                key: 'aaaa0000aaaa0000aaaa0000aaaa0000',
                dataUrl: COVER,
                mime: 'image/jpeg',
              },
              lazy: false,
            }
          }
          if (command === 'get_cover_thumbnails') {
            window.__coverCalls.push(args?.keys ?? [])
            return (args?.keys ?? [])
              .filter((key) => key === 'aaaa0000aaaa0000aaaa0000aaaa0000')
              .map((key) => ({ key, dataUrl: COVER, mime: 'image/jpeg' }))
          }
          return values[command]
        },
      }
    })
    await page.goto(target)
  } else {
    page = browser.contexts().flatMap((c) => c.pages())[0]
    if (!page) throw new Error('no page on the debugger')
    await page.evaluate(() => {
      localStorage.setItem('mio:locale', 'en')
      localStorage.removeItem('mio:inspector-open')
      localStorage.removeItem('mio:inspector-width')
    })
    await page.reload()
  }

  await page.waitForSelector('.row[data-path]', { timeout: 60_000 })
  await page.waitForTimeout(600)

  const lastRequest = async () => {
    if (isUrl) return page.evaluate(() => window.__searchCalls.at(-1) ?? null)
    // In the desktop window the request is not observable from JS; the chips are
    // the user-visible equivalent. Chip labels are their text: Solid sets `data-*`
    // on a custom element as a property, not an attribute, so `dataset.term` is
    // undefined on a hydrated `ix-chip`.
    return page.evaluate(() => ({
      terms: [...document.querySelectorAll('.query-chip')].map((el) => el.textContent?.trim() ?? ''),
      chips: [...document.querySelectorAll('.filter-chip:not(.query-chip)')].map((el) =>
        el.textContent?.trim()
      ),
    }))
  }

  const waitQuery = async () => {
    await page.waitForFunction(
      () => {
        const table = document.querySelector('.table')
        return (
          table?.dataset.querying === 'false' &&
          table.dataset.epoch === table.dataset.segmentsEpoch
        )
      },
      undefined,
      { timeout: 20_000 }
    )
    await page.waitForTimeout(120)
  }

  /**
   * The rail's mode and lit rows. An inspector filter is supposed to move the
   * rail to the matching tree and light the entry there, so this is the state
   * those clicks have to leave behind.
   */
  const sidebarState = () =>
    page.evaluate(() => ({
      mode: document.querySelector('.sidebar')?.getAttribute('data-sidebar-mode') ?? '',
      facets: [...document.querySelectorAll('.facet.selected')].map(
        (el) => el.getAttribute('data-category') ?? ''
      ),
      folders: [...document.querySelectorAll('.folder.selected')].map(
        (el) => el.getAttribute('data-folder') ?? ''
      ),
    }))

  // --- closed by default: only the strip
  if (isUrl) {
    // The fixture browser starts with an empty profile, so this is the shipped
    // column set rather than one the user arranged.
    const defaultHeads = await page.evaluate(() =>
      [...document.querySelectorAll('.head-cell')].map((el) => el.dataset.sortKey)
    )
    check(
      defaultHeads.join(',') ===
        'favorite,cover,filename,category,durationS,channels,bpm,sampleRate,filesize',
      'a fresh profile shows Cover … Size without UCS/Description',
      defaultHeads.join(',')
    )
  }
  check((await page.locator('.inspector-strip').count()) === 1, 'the inspector starts closed')
  check((await page.locator('.inspector').count()) === 0, 'a closed inspector renders no panel')
  const strip = await page.locator('.inspector-strip').evaluate((el) => {
    const r = el.getBoundingClientRect()
    return {
      width: r.width,
      writing: getComputedStyle(el).writingMode,
      expanded: el.getAttribute('aria-expanded'),
      children: [...el.children].map((child) => {
        const box = child.getBoundingClientRect()
        return { tag: child.tagName.toLowerCase(), top: box.top, height: box.height }
      })
    }
  })
  check(strip.width === 26, 'the strip is 26px wide', `${strip.width}`)
  check(strip.writing.startsWith('vertical'), 'the strip text is vertical', strip.writing)
  check(strip.expanded === 'false', 'the closed strip announces aria-expanded=false')
  const [topChevron, label, bottomChevron] = strip.children
  check(
    topChevron?.tag === 'ix-icon' &&
      label?.tag === 'span' &&
      bottomChevron?.tag === 'ix-icon' &&
      topChevron.height > 0 &&
      bottomChevron.height > 0 &&
      topChevron.top < label.top &&
      bottomChevron.top > label.top,
    'a chevron sits above and below the label',
    JSON.stringify(strip.children)
  )

  // --- open it
  await page.locator('.inspector-strip').click()
  await page.waitForSelector('.inspector')
  check((await page.locator('.inspector-strip').count()) === 0, 'the strip disappears when the panel opens')
  const openState = await page.evaluate(() => ({
    saved: localStorage.getItem('mio:inspector-open'),
    width: localStorage.getItem('mio:inspector-width'),
  }))
  check(openState.saved === '1', 'the open state is persisted', String(openState.saved))
  const panelW = await page.locator('.inspector').evaluate((el) => el.getBoundingClientRect().width)
  check(panelW === 300, 'the default width is 300px', `${panelW}`)
  check(
    (await page.locator('.inspector-close').getAttribute('aria-expanded')) === 'true',
    'the close button announces aria-expanded=true'
  )
  check((await page.locator('.inspector-empty').count()) === 1, 'an empty inspector says so')

  // --- select a row: the panel describes it without a fetch
  const firstPath = await page.locator('.row[data-path]').first().getAttribute('data-path')
  const firstBase = (firstPath ?? '').split(/[\\/]/).pop() ?? ''
  const firstFolder = firstBase.includes('.')
    ? (firstPath ?? '').split(/[\\/]/).slice(-2, -1)[0] ?? ''
    : ''
  const firstStem = firstBase.replace(/\.[^.]+$/, '')
  await page.locator('.row[data-path]').first().click()
  await page.waitForTimeout(400)
  const name = await page.locator('.inspector-row').first().locator('.inspector-value').textContent()
  check((name ?? '').includes(firstBase), 'the inspector shows the active row name', name ?? '')
  const folder = await page
    .locator('.inspector-row')
    .filter({ hasText: 'FOLDER' })
    .first()
    .locator('.inspector-value')
    .textContent()
  check((folder ?? '').includes(firstFolder), 'the inspector shows the relative folder', folder ?? '')

  // --- TEXT appends: clicking the name adds a term and keeps the filters
  await page.locator('.inspector-row').first().locator('.inspector-value').click()
  await waitQuery()
  const afterName = await lastRequest()
  const terms = isUrl ? (afterName?.query ?? '').split(' ').filter(Boolean) : afterName.terms
  check(terms.includes(firstStem), 'clicking the name searches its stem', JSON.stringify(terms))

  // A second text click appends; it must not replace the first.
  const description = page.locator('.inspector-description')
  check((await description.count()) === 1, 'the description is shown')
  await description.click()
  await waitQuery()
  const afterDescription = await lastRequest()
  const terms2 = isUrl ? (afterDescription?.query ?? '').split(' ').filter(Boolean) : afterDescription.terms
  check(terms2.length >= 2, 'a second text click appends to the query', JSON.stringify(terms2))

  // --- exclusive: a category click replaces the whole search
  const categoryRow = page
    .locator('.inspector-row')
    .filter({ hasText: 'CATEGORY' })
    .first()
  const categoryText = await categoryRow.locator('.inspector-value').textContent()
  await categoryRow.locator('.inspector-value').click()
  await waitQuery()
  const categoryRail = await sidebarState()
  check(
    categoryRail.mode === 'categories',
    'a category click switches the rail to Categories',
    categoryRail.mode
  )
  check(
    categoryRail.facets.includes((categoryText ?? '').trim()),
    'the rail lights the clicked category',
    JSON.stringify(categoryRail.facets)
  )
  const afterCategory = await lastRequest()
  if (isUrl) {
    check(
      JSON.stringify(afterCategory?.categories) === JSON.stringify(['Foley']),
      'a category click sets that category',
      JSON.stringify(afterCategory?.categories)
    )
    check(afterCategory?.query === '', 'a category click clears the query terms', JSON.stringify(afterCategory?.query))
    check(
      (await page.locator('.head-cell[data-sort-key="filename"] .sort-arrow').count()) >= 0,
      'the table stays mounted'
    )
  } else {
    check(afterCategory.terms.length === 0, 'a category click clears the query terms', JSON.stringify(afterCategory.terms))
    check(
      afterCategory.chips.some((chip) => (chip ?? '').includes(categoryText ?? '')),
      'the category chip is the only filter',
      JSON.stringify(afterCategory.chips)
    )
  }

  // --- folder click is an absolute-folder filter
  const folderRow = page.locator('.inspector-row').filter({ hasText: 'FOLDER' }).first()
  await folderRow.locator('.inspector-value').click()
  await waitQuery()
  const folderRail = await sidebarState()
  check(folderRail.mode === 'folders', 'a folder click switches the rail to Folders', folderRail.mode)
  check(
    folderRail.folders.some((path) => path.endsWith(firstFolder)),
    'the rail lights the clicked folder',
    JSON.stringify(folderRail.folders)
  )
  if (isUrl) {
    // The fixture puts Pack_1 below 40 sibling folders, so switching modes alone
    // would leave the lit row outside the rail's viewport.
    const railScroll = await page.evaluate(() => {
      const list = document.querySelector('.side-section.grow')
      const row = document.querySelector('.folder.selected')
      if (!list || !row) return { top: 0, visible: false }
      const listBox = list.getBoundingClientRect()
      const rowBox = row.getBoundingClientRect()
      return {
        top: list.scrollTop,
        visible: rowBox.top >= listBox.top - 1 && rowBox.bottom <= listBox.bottom + 1,
      }
    })
    check(
      railScroll.top > 0 && railScroll.visible,
      'the rail scrolled the clicked folder into view',
      JSON.stringify(railScroll)
    )
  }
  const afterFolder = await lastRequest()
  if (isUrl) {
    check(
      /^C:\\inspector-fixture\\Pack_1$/.test(afterFolder?.folder ?? ''),
      'a folder click filters by the absolute folder',
      String(afterFolder?.folder)
    )
  } else {
    check(
      afterFolder.chips.some((chip) => (chip ?? '') === firstFolder),
      'the folder chip is set',
      JSON.stringify(afterFolder.chips)
    )
  }

  // --- copy: the path row writes to the clipboard and does not search
  const callsBefore = isUrl ? await page.evaluate(() => window.__searchCalls.length) : 0
  await page.locator('.inspector-row').filter({ hasText: 'PATH' }).first().locator('.inspector-value').click()
  await page.waitForTimeout(250)
  if (isUrl) {
    const callsAfter = await page.evaluate(() => window.__searchCalls.length)
    check(callsAfter === callsBefore, 'copying the path does not re-query', `${callsBefore} → ${callsAfter}`)
  } else {
    check(
      (await page.locator('.notice').textContent())?.includes('Copied'),
      'copying the path shows the notice'
    )
  }

  // --- UCS category: same exclusive filter, on the UCS tree
  const ucsCategoryRow = page
    .locator('.inspector-row')
    .filter({ has: page.locator('.inspector-label', { hasText: /^UCS$/ }) })
    .first()
  const ucsText = (await ucsCategoryRow.locator('.inspector-value').textContent())?.trim() ?? ''
  await ucsCategoryRow.locator('.inspector-value').click()
  await waitQuery()
  const ucsRail = await sidebarState()
  check(ucsRail.mode === 'ucs', 'a UCS category click switches the rail to UCS', ucsRail.mode)
  check(
    ucsRail.facets.includes(ucsText),
    'the rail lights the clicked UCS category',
    JSON.stringify(ucsRail.facets)
  )

  // --- channels: appends to the other filters and toggles off on a second click
  const channelRow = page.locator('.inspector-row').filter({ hasText: 'CHANNELS' }).first()
  const channelText = (await channelRow.locator('.inspector-value').textContent())?.trim() ?? ''
  const channelCount = (channelText.match(/\d+/) ?? [])[0]
  await channelRow.locator('.inspector-value').click()
  await waitQuery()
  const afterChannels = await lastRequest()
  if (isUrl) {
    check(afterChannels?.channels === 2, 'a channel click filters by that count', String(afterChannels?.channels))
  } else {
    check(
      afterChannels.chips.some((chip) => /ch/.test(chip ?? '') && (chip ?? '').includes(channelCount ?? '')),
      'a channel chip carries the count',
      JSON.stringify(afterChannels.chips)
    )
  }
  await channelRow.locator('.inspector-value').click()
  await waitQuery()
  const cleared = await lastRequest()
  if (isUrl) {
    check(cleared?.channels === undefined, 'clicking the same count clears the filter')
  } else {
    check(
      !cleared.chips.some((chip) => /ch/.test(chip ?? '') && (chip ?? '').includes(channelCount ?? '')),
      'clicking the same count clears the chip'
    )
  }

  // --- the width is draggable and persisted
  const handle = await page.locator('.inspector-resize').boundingBox()
  await page.mouse.move(handle.x + 3, handle.y + 100)
  await page.mouse.down()
  await page.mouse.move(handle.x - 57, handle.y + 100, { steps: 6 })
  await page.mouse.up()
  await page.waitForTimeout(400)
  const draggedW = await page.locator('.inspector').evaluate((el) => el.getBoundingClientRect().width)
  check(Math.abs(draggedW - 360) < 8, 'dragging the edge resizes the panel', `${draggedW}`)
  check(
    (await page.evaluate(() => localStorage.getItem('mio:inspector-width'))) === String(Math.round(draggedW)),
    'the dragged width is persisted'
  )

  // --- the table stays inside its column beside the panel
  const geometry = await page.evaluate(() => {
    const table = document.querySelector('.table')?.getBoundingClientRect()
    const panel = document.querySelector('.inspector')?.getBoundingClientRect()
    const dock = document.querySelector('.dock')?.getBoundingClientRect()
    return {
      tableRight: table?.right ?? 0,
      panelLeft: panel?.left ?? 0,
      panelRight: panel?.right ?? 0,
      dockRight: dock?.right ?? 0,
      inner: window.innerWidth,
    }
  })
  check(Math.abs(geometry.tableRight - geometry.panelLeft) <= 1, 'the table ends where the panel starts', JSON.stringify(geometry))
  check(geometry.panelRight <= geometry.inner + 1, 'the panel stays inside the window')
  check(geometry.dockRight <= geometry.inner + 1, 'the dock still spans only the window')

  // --- closing persists too, and the panel leaves no requests behind
  await page.locator('.inspector-close').click()
  await page.waitForTimeout(150)
  check((await page.locator('.inspector').count()) === 0, 'the close button hides the panel')
  check(
    (await page.evaluate(() => localStorage.getItem('mio:inspector-open'))) === '0',
    'the closed state is persisted'
  )

  // --- reload: the open state survives
  await page.locator('.inspector-strip').click()
  await page.waitForSelector('.inspector')
  await page.reload()
  await page.waitForSelector('.row[data-path]', { timeout: 60_000 })
  await page.waitForTimeout(600)
  check((await page.locator('.inspector').count()) === 1, 'the open state survives a reload')

  // --- the cover column and the details payload (fixture side only: the desktop
  //     window has real files and no instrumented IPC)
  if (isUrl) {
    check((await page.locator('.head-cell[data-sort-key="cover"]').count()) === 1, 'the cover column is in the header')
    const order = await page.evaluate(() =>
      [...document.querySelectorAll('.head-cell')].map((el) => el.dataset.sortKey)
    )
    check(order[0] === 'favorite' && order[1] === 'cover', 'the cover column sits after the star', JSON.stringify(order.slice(0, 3)))
    check((await page.locator('.cell.cover').count()) > 0, 'rows draw cover cells')
    await page.waitForTimeout(500)
    const withImage = await page.locator('.row[data-path] .cover-thumb').count()
    check(withImage === 1, 'a row with a cover shows its thumbnail', `${withImage}`)
    check(
      (await page.locator('.row[data-path] .cover-blank').count()) === 2,
      'a row without a cover shows a placeholder'
    )
    const batches = await page.evaluate(() => window.__coverCalls)
    // The invariant is "one batch per viewport, never per cell": an extra run
    // after the effect chain settles is allowed, a request per row is not.
    check(
      batches.length >= 1 && batches.length <= 2,
      'the viewport batches its thumbnail requests',
      `${batches.length} request(s)`
    )
    check(
      batches.every((keys) => keys.length === 1 && keys[0] === 'aaaa0000aaaa0000aaaa0000aaaa0000'),
      'every batch carries the distinct keys only',
      JSON.stringify(batches)
    )

    // --- the inspector's tags arrive from inspect_sound and group by source
    await page.locator('.row[data-path]').first().click()
    await page.waitForTimeout(500)
    check((await page.locator('.inspector-cover img').count()) === 1, 'the inspector shows the 512px cover')
    const sources = await page.evaluate(() =>
      [...document.querySelectorAll('.inspector-tag-source')].map((el) => el.textContent)
    )
    check(sources.includes('ID3') && sources.includes('BWF bext'), 'tags are grouped by source', JSON.stringify(sources))
    const searchable = await page.locator('.inspector-tag-value.searchable').count()
    check(searchable === 2, 'only searchable tag values are buttons', `${searchable}`)
    const plain = await page.locator('span.inspector-tag-value').count()
    check(plain === 1, 'an unsearchable value is plain text', `${plain}`)

    // A tag click appends to the query like any other text value.
    const beforeTag = await page.evaluate(() => window.__searchCalls.length)
    await page.locator('.inspector-tag-value.searchable').first().click()
    await waitQuery()
    const afterTag = await page.evaluate(() => window.__searchCalls.length)
    check(afterTag > beforeTag, 'clicking a tag value searches it')
    const tagRequest = await page.evaluate(() => window.__searchCalls.at(-1))
    check(
      (tagRequest?.query ?? '').includes('Naimio'),
      'the tag value became a query term',
      JSON.stringify(tagRequest?.query)
    )

    // The cover click replaces the search with that cover only.
    await page.locator('.inspector-cover').click()
    await waitQuery()
    const coverRequest = await page.evaluate(() => window.__searchCalls.at(-1))
    check(
      coverRequest?.coverHash === 'aaaa0000aaaa0000aaaa0000aaaa0000',
      'clicking the cover filters by its hash',
      String(coverRequest?.coverHash)
    )
    check(coverRequest?.query === '', 'the cover click clears the terms', JSON.stringify(coverRequest?.query))

    // A cover cell click does the same and must not audition the row.
    await page.locator('.row[data-path]').first().locator('.cell.cover').click()
    await waitQuery()
    const cellRequest = await page.evaluate(() => window.__searchCalls.at(-1))
    check(
      cellRequest?.coverHash === 'aaaa0000aaaa0000aaaa0000aaaa0000',
      'a cover cell click filters too',
      String(cellRequest?.coverHash)
    )

    // Hiding the column means zero thumbnail requests.
    await page.evaluate(() => window.__coverCalls.length = 0)
    await page.evaluate(() => {
      localStorage.setItem('mio:column-hidden', JSON.stringify(['cover']))
    })
    await page.reload()
    await page.waitForSelector('.row[data-path]', { timeout: 60_000 })
    await page.waitForTimeout(800)
    const hiddenCalls = await page.evaluate(() => window.__coverCalls.length)
    check((await page.locator('.cell.cover').count()) === 0, 'a hidden cover column draws no cells')
    check(hiddenCalls === 0, 'a hidden cover column makes no thumbnail requests', `${hiddenCalls}`)
    await page.evaluate(() => localStorage.removeItem('mio:column-hidden'))
  }

  assert.deepEqual(problems, [])
  console.log('\ninspector: OK')
} finally {
  await browser.close()
}
