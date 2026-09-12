export const THEME_PRESET_KEY = 'pertisk_theme_preset'
export const THEME_APPEARANCE_KEY = 'pertisk_theme_appearance'

const CONSOLE_TERM_DARK = {
  background: '#17132a',
  foreground: '#e7e1f5',
  cursor: '#b79cff',
  cursorAccent: '#17132a',
  selectionBackground: '#4c3f7a',
  black: '#2a2440',
  red: '#ff7b72',
  green: '#8ce39b',
  yellow: '#f0d98c',
  blue: '#a78bfa',
  magenta: '#c792ea',
  cyan: '#89ddff',
  white: '#e7e1f5',
  brightBlack: '#6b6390',
  brightRed: '#ff8a84',
  brightGreen: '#7aecb8',
  brightYellow: '#fcd34d',
  brightBlue: '#c4b5fd',
  brightMagenta: '#e0c4ff',
  brightCyan: '#a5f3fc',
  brightWhite: '#ffffff',
}

const LEGACY_INLINE_TOKENS = [
  '--color-primary-p1',
  '--color-primary-p2',
  '--color-primary-p3',
  '--color-primary-p4',
  '--color-primary-p5',
  '--color-primary-p6',
  '--color-primary',
  '--color-primary-hover',
  '--color-sidebar',
  '--color-bg',
  '--color-surface',
  '--color-surface-elevated',
  '--color-hover',
  '--color-border',
  '--color-card',
  '--color-text',
  '--color-text-secondary',
  '--color-muted',
  '--color-bg-gradient-start',
  '--color-bg-gradient-end',
  '--color-icon-primary',
  '--color-dashboard-metric-secondary',
  '--color-dashboard-metric-secondary-bg',
  '--color-workload-accent',
  '--color-workload-accent-strong',
]

export function normalizeAppearance(value) {
  return value === 'light' ? 'light' : 'dark'
}

export function getStoredAppearance() {
  try {
    return normalizeAppearance(localStorage.getItem(THEME_APPEARANCE_KEY))
  } catch {
    return 'dark'
  }
}

function paintTheme(appearance) {
  const mode = normalizeAppearance(appearance)
  const root = document.documentElement
  root.classList.toggle('dark', mode === 'dark')
  root.classList.toggle('light', mode === 'light')
  root.style.colorScheme = mode
  root.removeAttribute('data-theme')
  for (const key of LEGACY_INLINE_TOKENS) {
    root.style.removeProperty(key)
  }
  try {
    localStorage.removeItem(THEME_PRESET_KEY)
    localStorage.removeItem('theme')
  } catch {
    /* ignore quota / private mode */
  }
  return mode
}

export function applyAppearance(appearance) {
  const mode = paintTheme(appearance)
  try {
    localStorage.setItem(THEME_APPEARANCE_KEY, mode)
  } catch {
    /* ignore quota / private mode */
  }
  return mode
}

export function initTheme() {
  paintTheme(getStoredAppearance())
}

export function terminalPalette(appearance) {
  void appearance
  return CONSOLE_TERM_DARK
}
