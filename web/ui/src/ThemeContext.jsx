import { createContext, useContext, useMemo, useState } from 'react'
import {
  APP_THEME_PRESETS,
  applyAppearance,
  applyThemePreset,
  getStoredAppearance,
  getStoredThemePreset,
  terminalPalette,
} from './theme'

const ThemeContext = createContext(null)

export function ThemeProvider({ children }) {
  const [preset, setPresetState] = useState(getStoredThemePreset)
  const [appearance, setAppearanceState] = useState(getStoredAppearance)

  const value = useMemo(() => {
    function setPreset(next) {
      setPresetState(applyThemePreset(next))
    }
    function setAppearance(next) {
      setAppearanceState(applyAppearance(next))
    }
    function toggleAppearance() {
      setAppearanceState(applyAppearance(appearance === 'dark' ? 'light' : 'dark'))
    }
    return {
      preset,
      appearance,
      presets: APP_THEME_PRESETS,
      setPreset,
      setAppearance,
      toggleAppearance,
      terminalTheme: terminalPalette(preset, appearance),
    }
  }, [preset, appearance])

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) {
    throw new Error('useTheme must be used within ThemeProvider')
  }
  return ctx
}
