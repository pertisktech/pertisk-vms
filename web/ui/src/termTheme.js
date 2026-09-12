import { getStoredAppearance, terminalPalette } from './theme'

/** Terminal palette for the console violet theme. */
export function getXtermTheme() {
  return terminalPalette(getStoredAppearance())
}
