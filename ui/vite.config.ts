import { defineConfig, type Plugin } from 'vite'
import solid from 'vite-plugin-solid'

// Tauri's dev server port. Fixed rather than random so `tauri.conf.json`'s
// devUrl always matches; a random port would make every restart a config edit.

/**
 * The iX elements the app actually renders, as tag names.
 *
 * `defineCustomElements` ships metadata for every component in the design
 * system (~100 tags, ~1MB of lazy entry chunks), and the exe embeds dist
 * wholesale. `trimIxChunks` below rewrites the built loader so the entries of
 * everything not on this list are gone. Adding an `<ix-…>` element to a
 * component means adding its tag here or it will silently stay an unknown
 * element.
 *
 * **The failure is silent at runtime beyond one console line**: the loader's
 * dynamic import is pointed at `data:text/javascript,`, the page's CSP blocks
 * that, and every instance logs `Constructor for "ix-x#undefined" was not
 * found` and never upgrades. A trimmed checkbox therefore renders as an empty
 * inline element — a section that looks blank, not a missing control.
 *
 * The dispatch switch is keyed by *bundle id*, not tag: a lazy bundle that
 * defines several tags (ix-icon-button + ix-spinner) has a `name_2` id, and
 * every tag a bundle contains must be listed or the whole bundle is trimmed.
 * `ix-icon` itself comes from the separate `@siemens/ix-icons` loader and has
 * its own dispatch, so it is listed for clarity but is never part of this
 * switch.
 */
const IX_TAGS = [
  'ix-button',
  'ix-checkbox',
  'ix-chip',
  'ix-icon',
  // bundle id of ix-icon-button + ix-spinner
  'ix-icon-button_2',
  'ix-icon-toggle-button',
  'ix-input',
  'ix-modal',
  // bundle id of ix-modal-header + ix-modal-content
  'ix-modal-content_2',
  'ix-modal-footer',
  'ix-modal-header',
  'ix-progress-indicator',
  'ix-select',
  // bundle id of ix-select-item + ix-filter-chip
  'ix-filter-chip_2',
  'ix-select-item',
  'ix-slider',
  'ix-toggle',
  'ix-toggle-button',
  // Helper bundles the kept components render inside their own templates:
  // inputs/selects render ix-field-wrapper, buttons and chips render
  // ix-tooltip, the settings dialog renders ix-typography. Trim these and the
  // kept components throw "constructor not found" at upgrade time.
  'ix-typography',
  'ix-helper-text',
  'ix-tooltip',
  'ix-field-label_2',
  // bundle id of ix-dropdown-item + ix-dropdown: the selects' and the
  // category-dialog's dropdown lists render these as their light-DOM children.
  'ix-dropdown_2'
]

/**
 * Drop the lazy entry chunks of iX components the app never renders.
 *
 * The loader's `exclude` only stops registration at runtime; the bytes would
 * still sit in dist and get embedded into the exe. This reads the built entry's
 * import map and rewrites the dynamic `import()` specifiers for excluded
 * components to a shared no-op, so Rollup's tree-shaking pass over the chunk
 * graph is not enough on its own — hence the post-build rewrite.
 */
function trimIxChunks(): Plugin {
  return {
    name: 'trim-ix-chunks',
    apply: 'build',
    generateBundle(_, bundle) {
      // First pass: point the loader's dispatch at a no-op for every tag the
      // app does not render.
      for (const file of Object.values(bundle)) {
        if (file.type !== 'chunk') continue
        if (!/case\s*["']ix-/.test(file.code)) continue
        const used = new Set(IX_TAGS)
        // The dispatch reaches the bundle minified, shaped either as
        // `case 'ix-x': return import('./ix-x.entry-….js')` or, after esbuild,
        // `case "ix-x":return _(()=>import("./ix-x.entry-….js"),…)`. Both forms
        // are rewritten so an excluded tag imports a shared no-op module
        // instead.
        file.code = file.code.replace(
          /case\s*(['"])(ix-[a-z0-9_-]*)\1(\s*:\s*return\s*[_$a-zA-Z]*\(\s*\(\)\s*=>\s*import\s*\(\s*)(['"])([^'"]+)\4/g,
          (full, q1: string, tag: string, mid: string, q2: string, specifier: string) => {
            if (!specifier.includes('.entry')) return full
            if (used.has(tag)) return full
            return `case ${q1}${tag}${q1}${mid}${q2}data:text/javascript,${q2}`
          }
        )
      }
      // Second pass: the rewritten dispatch no longer references the excluded
      // entry chunks, so they are orphans — emit nothing for them. A bundle
      // file is kept iff its bundle id (the name before `.entry`, which for
      // multi-tag bundles ends in `_N`) is in the keep list.
      for (const file of Object.values(bundle)) {
        if (file.type !== 'chunk' || !file.fileName.includes('.entry-')) continue
        const id = /\/?(ix-[a-z0-9_-]+)\.entry-/.exec(file.fileName)?.[1]
        if (id && !IX_TAGS.includes(id)) delete bundle[file.fileName]
      }
    }
  }
}

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] }
  },
  build: {
    target: 'chrome110',
    outDir: 'dist',
    emptyOutDir: true,
    // Maps cost ~3.8MB embedded in the exe; nothing in production reads them.
    // `MIO_MAPS=1 npm run build` puts them back for a debugging session.
    sourcemap: process.env.MIO_MAPS === '1'
  },
  plugins: [solid(), trimIxChunks()]
})
