import { render } from 'solid-js/web'
import '@siemens/ix/dist/siemens-ix/siemens-ix.css'
import { defineCustomElements as defineIcons } from '@siemens/ix-icons/loader'
import { defineCustomElements } from '@siemens/ix/loader'
import './styles.css'
import { App } from './App'
import { LOCALES, locale } from './i18n'
import { applyTheme, installThemeDebugHook, loadThemeId } from './theme'

// iX is a set of web components: the CSS alone renders nothing, the element
// definitions have to be registered before the first `<ix-…>` tag is upgraded.
// Both registrations are synchronous enough that the app can render immediately
// after them; Stencil queues the upgrade per element.
void defineIcons()
void defineCustomElements()

// The theme goes on the document before the first paint. Doing it inside the app
// would flash iX's default palette for one frame, which is the one frame a theme
// must not get wrong.
applyTheme(loadThemeId())
// `<html lang>` decides the font stack Windows picks for CJK text and which IME a
// text field gets, so it has to follow the locale rather than stay `en`.
// `setLocale` keeps this up to date after a switch; this is only the first paint.
const entry = LOCALES.find((l) => l.id === locale())
if (entry) document.documentElement.lang = entry.htmlLang
// Exposes `window.__mioApplyTheme` for the CDP theme check. One assignment, no
// behaviour change: the settings dialog calls `applyTheme` directly.
installThemeDebugHook()

const root = document.getElementById('root')
if (!root) throw new Error('#root is missing from index.html')

render(() => <App />, root)
