import { createRequire } from 'node:module'
import { mkdir, rm, writeFile, unlink } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const port = process.argv[2] ?? '9223'
// Per-user temporary directory, not a hardcoded checkout path: this is test
// fixture data, and the script should run on any machine that has the app open.
const base = path.join(os.tmpdir(), 'opencode', 'mio-index-lifecycle')
const cancelRoot = path.join(base, 'cancel')
const failRoot = path.join(base, 'fail')
const wav = Buffer.from([
  0x52, 0x49, 0x46, 0x46, 0x2c, 0x01, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45,
  0x66, 0x6d, 0x74, 0x20, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00,
  0x44, 0xac, 0x00, 0x00, 0x10, 0xb1, 0x02, 0x00, 0x04, 0x00, 0x10, 0x00,
  0x64, 0x61, 0x74, 0x61, 0x00, 0x00, 0x00, 0x00
])

await rm(base, { recursive: true, force: true })
await mkdir(cancelRoot, { recursive: true })
await mkdir(failRoot, { recursive: true })
for (const [root, prefix, count] of [[cancelRoot, 'cancel', 3000], [failRoot, 'fail', 3000]]) {
  await Promise.all(Array.from({ length: count }, (_, index) =>
    writeFile(path.join(root, `${prefix}_${String(index).padStart(5, '0')}.wav`), wav)
  ))
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`)
const page = browser.contexts().flatMap((context) => context.pages())[0]
if (!page) throw new Error('no page on the debugger')

const invoke = (command, args) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
  { command, args }
)

const add = (root) => page.evaluate(async (library) => {
  const callback = window.__TAURI_INTERNALS__.transformCallback(() => {})
  return window.__TAURI_INTERNALS__.invoke('add_library', {
    root: library,
    full: true,
    onProgress: `__CHANNEL__:${callback}`
  })
}, root)

const cancelResult = await add(cancelRoot)
await invoke('remove_library', { root: cancelRoot })
const afterCancel = (await invoke('list_libraries')).find((library) => library.root === cancelRoot) ?? null

const failureResult = await add(failRoot)
const files = Array.from({ length: 1500 }, (_, index) =>
  path.join(failRoot, `fail_${String(index).padStart(5, '0')}.wav`)
)
await Promise.all(files.map((file) => unlink(file).catch(() => {})))

const samples = []
const deadline = Date.now() + 30_000
let final = null
while (Date.now() < deadline) {
  const status = (await invoke('list_libraries')).find((library) => library.root === failRoot) ?? null
  if (status) {
    samples.push({ indexing: status.metadataIndexing, indexed: status.metadataIndexed, total: status.metadataTotal, failed: status.metadataFailed, state: status.metadataStatus })
    final = status
    if (!status.metadataIndexing) break
  }
  await new Promise((resolve) => setTimeout(resolve, 100))
}
await invoke('remove_library', { root: failRoot })
await rm(base, { recursive: true, force: true })

console.log(JSON.stringify({
  cancel: { count: cancelResult.count, metadataIndexing: cancelResult.metadataIndexing, removed: afterCancel === null },
  failure: { count: failureResult.count, final, samples: samples.slice(0, 8), failed: (final?.metadataFailed ?? 0) > 0, completed: final?.metadataStatus === 'failed' }
}, null, 2))
await browser.close()
