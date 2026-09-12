import { createContext, useContext, useMemo, useState } from 'react'
import { applyAppearance, getStoredAppearance, terminalPalette } from './theme'

const ThemeContext = createContext(null)

export function ThemeProvider({ children }) {
  const [appearance, setAppearanceState] = useState(getStoredAppearance)

  const value = useMemo(() => {
    function setAppearance(next) {
      setAppearanceState(applyAppearance(next))
    }
    function toggleAppearance() {
      setAppearanceState(applyAppearance(appearance === 'dark' ? 'light' : 'dark'))
    }
    return {
      appearance,
      setAppearance,
      toggleAppearance,
      terminalTheme: terminalPalette(appearance),
    }
  }, [appearance])

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) {
    throw new Error('useTheme must be used within ThemeProvider')
  }
  return ctx
}
