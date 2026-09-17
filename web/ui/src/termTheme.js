import { getStoredAppearance, terminalPalette } from './theme'

/** xterm.js palette for the current UI appearance. */
export function getXtermTheme() {
  return terminalPalette(getStoredAppearance())
}

export function applyXtermTheme(term, host, palette) {
  if (!palette) return
  if (term) {
    term.options.theme = palette
    try {
      term.refresh(0, Math.max(0, (term.rows || 1) - 1))
    } catch {
      /* disposed */
    }
  }
  if (host) host.style.background = palette.background
}
