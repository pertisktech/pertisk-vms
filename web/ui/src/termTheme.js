import { getStoredThemePreset, terminalPalette } from './theme'

/** Terminal palette that follows the current app color theme. */
export function getXtermTheme(presetId) {
  return terminalPalette(presetId || getStoredThemePreset())
}
