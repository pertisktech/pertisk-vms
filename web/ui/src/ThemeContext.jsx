import { createContext, useContext, useMemo, useState } from 'react'
import {
  APP_THEME_PRESETS,
  applyThemePreset,
  getStoredThemePreset,
  terminalPalette,
} from './theme'

const ThemeContext = createContext(null)

export function ThemeProvider({ children }) {
  const [preset, setPresetState] = useState(getStoredThemePreset)

  const value = useMemo(() => {
    function setPreset(next) {
      setPresetState(applyThemePreset(next))
    }
    return {
      preset,
      presets: APP_THEME_PRESETS,
      setPreset,
      terminalTheme: terminalPalette(preset),
    }
  }, [preset])

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) {
    throw new Error('useTheme must be used within ThemeProvider')
  }
  return ctx
}
