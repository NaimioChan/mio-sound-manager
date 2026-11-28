// Second §8.2 pass: a full 33-key viewport, FTS tag search, and the plans.
import { createRequire } from 'node:module'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const hashes = process.argv[2].split(',')
const tagTerm = process.argv[3] ?? 'Vintage'
const browser = await chromium.connectOverCDP('http://127.0.0.1:9223')
const page = browser.contexts().flatMap((c) => c.pages())[0]
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args])

const pct = (xs, q) => {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.floor(s.length * q))]
}
const round = (x) => Math.round(x * 100) / 100

const times = []
let bytes = 0
for (let i = 0; i < 15; i++) {
  const t0 = performance.now()
  const answer = await invoke('get_cover_thumbnails', { keys: hashes })
  times.push(performance.now() - t0)
  if (i === 0) bytes = answer.reduce((n, t) => n + (t.dataUrl?.length ?? 0), 0)
}
console.log(
  '33-key batch: returned=%d p50=%sms p95=%sms payload=%sKB',
  hashes.length,
  round(pct(times, 0.5)),
  round(pct(times, 0.95)),
  Math.round(bytes / 1024)
)

// The FTS tags column: a term that exists only in tag text must be findable.
const tagCount = await invoke('count_matching', { request: { query: tagTerm, limit: 0 } })
const filenameCount = await invoke('count_matching', {
  request: { query: 'zzqv-no-such-name', limit: 0 },
})
console.log('fts tag term %o -> %d rows (control %d)', tagTerm, tagCount, filenameCount)

// Query plans from the live connection's own SQLite, via a read-only handle is not
// available over IPC, so measure instead: p95 over repeats.
for (const [label, request] of [
  ['cover filter', { query: '', coverHash: hashes[0], limit: 50 }],
  ['cover sort', { query: '', sortBy: 'cover', sortDir: 'asc', limit: 50 }],
  ['tag query', { query: tagTerm, limit: 50 }],
]) {
  const runs = []
  for (let i = 0; i < 10; i++) {
    const t0 = performance.now()
    await invoke('search', { request })
    runs.push(performance.now() - t0)
  }
  console.log('%s: p50=%sms p95=%sms', label, round(pct(runs, 0.5)), round(pct(runs, 0.95)))
}

await browser.close()
