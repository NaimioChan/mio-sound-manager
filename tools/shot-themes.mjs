// Screenshot every theme, for looking at rather than asserting on.
//
//   node tools/shot-themes.mjs [outDir]
import { createRequire } from 'node:module'
import { mkdirSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const outDir = process.argv[2] ?? path.join(os.tmpdir(), 'mio-themes')
mkdirSync(outDir, { recursive: true })

const browser = await chromium.connectOverCDP('http://127.0.0.1:9223')
const page = browser.contexts().flatMap((c) => c.pages())[0]
await page.reload()
await page.waitForSelector('.row', { timeout: 60_000 })
await page.waitForTimeout(1500)

const ids = [
  'windows-dark',
  'windows-light',
  'studio-dark',
  'studio-light',
  'amber-console',
  'frost-dark',
  'tokyo-night'
]
for (const id of ids) {
  await page.evaluate((id) => window.__mioApplyTheme(id), id)
  await page.waitForTimeout(500)
  const path = `${outDir}/${id}.png`
  await page.screenshot({ path })
  console.log(path)
}
// Leave the window in the default theme.
await page.evaluate(() => window.__mioApplyTheme('windows-dark'))
await browser.close()
