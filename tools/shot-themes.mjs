// Screenshot every theme, for looking at rather than asserting on.
//
//   node tools/shot-themes.mjs [outDir]
import { createRequire } from 'node:module'
import { mkdirSync } from 'node:fs'

const require = createRequire('C:/Users/Naimio/projects/slip-sound/package.json')
const { chromium } = require('playwright-core')

const outDir = process.argv[2] ?? 'C:/Users/Naimio/AppData/Local/hermes/cache/scratch/themes'
mkdirSync(outDir, { recursive: true })

const browser = await chromium.connectOverCDP('http://127.0.0.1:9223')
const page = browser.contexts().flatMap((c) => c.pages())[0]
await page.reload()
await page.waitForSelector('.row', { timeout: 60_000 })
await page.waitForTimeout(1500)

const ids = ['classic-dark', 'classic-light', 'studio-dark', 'studio-light', 'amber-console', 'frost-dark']
for (const id of ids) {
  await page.evaluate((id) => window.__mioApplyTheme(id), id)
  await page.waitForTimeout(500)
  const path = `${outDir}/${id}.png`
  await page.screenshot({ path })
  console.log(path)
}
// Leave the window in the default theme.
await page.evaluate(() => window.__mioApplyTheme('classic-dark'))
await browser.close()
