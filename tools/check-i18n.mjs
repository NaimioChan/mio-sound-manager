// Locale consistency check.
//
//   node tools/check-i18n.mjs
//
// Three things `tsc` cannot catch, all of which ship as a wrong label rather than a
// build error:
//
//   1. A translation that drops a `{placeholder}` the English text has. The result
//      renders as a sentence with a number missing — "3 个" instead of "3 个声音".
//   2. A translation that invents a placeholder English does not have, which
//      renders as a literal `{count}` on screen.
//   3. A locale file that is missing a key entirely. `Record<MessageKey, string>`
//      does catch this, but only if the file is type-checked; this script also runs
//      on a locale added without touching `i18n.ts` yet.
//
// It reads the TypeScript source with a regex rather than importing it, so it needs
// no build step and no ts-node: the locale files are flat `'key': 'value',` lists by
// construction, and that shape is what the check enforces.
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

// Resolved from this file, not the process's working directory: `AGENTS.md`
// documents the check as `node tools/check-i18n.mjs` from the repository root,
// and a relative `src/locales` made that fail with ENOENT. Every other
// `check-*.mjs` resolves its own paths the same way.
const HERE = dirname(fileURLToPath(import.meta.url))
const dir = join(HERE, '..', 'ui', 'src', 'locales')
const files = readdirSync(dir).filter((f) => f.endsWith('.ts'))
const problems = []

/** `'key.name': 'text'` → Map(key → text). Tolerates both quote styles. */
function parse(file) {
  const src = readFileSync(join(dir, file), 'utf8')
  const map = new Map()
  const re = /^\s*'([\w.]+)':\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)"),?\s*$/gm
  let m
  while ((m = re.exec(src))) {
    map.set(m[1], (m[2] ?? m[3] ?? '').replace(/\\(['"\\])/g, '$1'))
  }
  return map
}

const dicts = new Map(files.map((f) => [f, parse(f)]))
const en = dicts.get('en.ts')
if (!en) {
  console.error('src/locales/en.ts is missing; it is the key-set reference')
  process.exit(1)
}

const placeholders = (text) => new Set([...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]))

for (const [file, dict] of dicts) {
  if (file === 'en.ts') continue
  const label = file.replace(/\.ts$/, '')

  for (const key of en.keys()) {
    if (!dict.has(key)) problems.push(`${label}: missing key '${key}'`)
  }
  for (const key of dict.keys()) {
    if (!en.has(key)) problems.push(`${label}: key '${key}' is not in en.ts`)
  }

  for (const [key, text] of dict) {
    const source = en.get(key)
    if (source === undefined) continue
    const want = placeholders(source)
    const got = placeholders(text)
    for (const name of want) {
      if (!got.has(name)) problems.push(`${label}: '${key}' dropped {${name}}`)
    }
    for (const name of got) {
      if (!want.has(name)) problems.push(`${label}: '${key}' invented {${name}}`)
    }
  }

  // An untranslated value is not always a bug ("OK", product names), so this is a
  // warning rather than a failure — but it is worth seeing.
  const identical = [...dict].filter(([k, v]) => en.get(k) === v).map(([k]) => k)
  if (identical.length > 0) {
    console.log(`note: ${label} has ${identical.length} value(s) identical to English:`)
    for (const key of identical) console.log(`  ${key}`)
  }
}

if (problems.length > 0) {
  console.error(`\ni18n: ${problems.length} problem(s)`)
  for (const p of problems) console.error(`  ${p}`)
  process.exit(1)
}

const counts = [...dicts].map(([f, d]) => `${f.replace(/\.ts$/, '')}=${d.size}`).join(' ')
console.log(`i18n: OK (${counts})`)
