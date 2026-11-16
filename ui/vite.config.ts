import { defineConfig } from 'vite'
import solid from 'vite-plugin-solid'

// Tauri's dev server port. Fixed rather than random so `tauri.conf.json`'s
// devUrl always matches; a random port would make every restart a config edit.
export default defineConfig({
  plugins: [solid()],
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
    sourcemap: true
  }
})
