// §8.2 measurements on a real-size library: thumbnail batches, inspect_sound,
// and the LRU hit/miss split. Read-only against the open window.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')
;(async () => {
  const b = await chromium.connectOverCDP('http://127.0.0.1:9223')
  const p = b.contexts().flatMap((c) => c.pages())[0]
  await p.evaluate(() => localStorage.setItem('mio:locale', 'en'))
  await p.reload()
  await p.waitForSelector('.row[data-path]', { timeout: 90000 })
  await p.waitForFunction(
    () => document.querySelector('.table')?.dataset.querying === 'false',
    undefined,
    { timeout: 60000 }
  )
  await p.waitForTimeout(1500)

  const invoke = (cmd, args) =>
    p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args])

  const pct = (xs, q) => {
    if (!xs.length) return null
    const s = [...xs].sort((a, b) => a - b)
    return s[Math.min(s.length - 1, Math.floor(s.length * q))]
  }
  const round = (x) => (x === null ? null : Math.round(x * 100) / 100)

  // The visible window's distinct cover keys, exactly what the table asks for.
  const keys = await p.evaluate(async () => {
    const rows = [...document.querySelectorAll('.row[data-path]')]
    const out = new Set()
    for (const row of rows) {
      const path = row.getAttribute('data-path')
      if (!path) continue
      const found = await window.__TAURI_INTERNALS__.invoke('search', {
        request: { query: '', limit: 1, offset: Number(row.dataset.index) }
      })
      const key = found?.[0]?.coverKey
      if (key) out.add(key)
    }
    return [...out]
  })
  console.log('visible distinct cover keys', keys.length)

  // One-screen batch, repeated to see the spread.
  const batchTimes = []
  let bytes = 0
  for (let i = 0; i < 12; i++) {
    const t0 = performance.now()
    const answer = await invoke('get_cover_thumbnails', { keys })
    batchTimes.push(performance.now() - t0)
    if (i === 0) bytes = answer.reduce((n, t) => n + (t.dataUrl?.length ?? 0), 0)
  }
  console.log(
    'thumbnails: n=%d p50=%sms p95=%sms payload=%sKB',
    keys.length,
    round(pct(batchTimes, 0.5)),
    round(pct(batchTimes, 0.95)),
    Math.round(bytes / 1024)
  )

  // inspect_sound: cold (first sight of a path) then warm (LRU).
  const paths = await p.evaluate(() =>
    [...document.querySelectorAll('.row[data-path]')].slice(0, 40).map((r) => r.getAttribute('data-path'))
  )
  const cold = []
  for (const path of paths) {
    const t0 = performance.now()
    await invoke('inspect_sound', { path })
    cold.push(performance.now() - t0)
  }
  const warm = []
  for (const path of paths) {
    const t0 = performance.now()
    await invoke('inspect_sound', { path })
    warm.push(performance.now() - t0)
  }
  console.log(
    'inspect_sound: cold p50=%sms p95=%sms | warm(LRU) p50=%sms p95=%sms',
    round(pct(cold, 0.5)),
    round(pct(cold, 0.95)),
    round(pct(warm, 0.5)),
    round(pct(warm, 0.95))
  )

  // Filter and sort plans on the real table.
  for (const [label, request] of [
    ['cover filter', { query: '', coverHash: keys[0] ?? 'none', limit: 50 }],
    ['sample rate filter', { query: '', sampleRate: 48000, limit: 50 }],
    ['cover sort', { query: '', sortBy: 'cover', sortDir: 'asc', limit: 50 }],
  ]) {
    const t0 = performance.now()
    const rows = await invoke('search', { request })
    const ms = performance.now() - t0
    const count = await invoke('count_matching', { request: { ...request, limit: 0 } })
    console.log('%s: %d rows in %sms, count=%d', label, rows.length, round(ms), count)
  }

  // A tag search that can only match the new FTS column.
  const tagHits = await invoke('count_matching', {
    request: { query: 'originator', limit: 0 },
  })
  console.log('tag-term count (fts tags column)', tagHits)

  await b.close()
})()
