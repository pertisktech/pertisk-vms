export const THEME_PRESET_KEY = 'pertisk_theme_preset'
export const THEME_APPEARANCE_KEY = 'pertisk_theme_appearance'

const CONSOLE_TERM_DARK = {
  background: '#12151c',
  foreground: '#e8eaef',
  cursor: '#9db4ff',
  cursorAccent: '#12151c',
  selectionBackground: '#2a3550',
  selectionInactiveBackground: '#1e2638',
  black: '#1a1e28',
  red: '#ff7b72',
  green: '#7dcea0',
  yellow: '#e6c07b',
  blue: '#7aa2f7',
  magenta: '#bb9af7',
  cyan: '#7dcfff',
  white: '#d5d8e0',
  brightBlack: '#6b7280',
  brightRed: '#ff8a84',
  brightGreen: '#8ee4b0',
  brightYellow: '#f0d98c',
  brightBlue: '#9db4ff',
  brightMagenta: '#d2b8ff',
  brightCyan: '#a5e8ff',
  brightWhite: '#ffffff',
}

const CONSOLE_TERM_LIGHT = {
  background: '#f7f8fa',
  foreground: '#1c2430',
  cursor: '#3b5bdb',
  cursorAccent: '#f7f8fa',
  selectionBackground: '#d7e0f5',
  selectionInactiveBackground: '#e7ecf6',
  black: '#1c2430',
  red: '#c23d48',
  green: '#217a4c',
  yellow: '#8f6410',
  blue: '#3451b2',
  magenta: '#7a4ea3',
  cyan: '#147a8e',
  white: '#d5d8e0',
  brightBlack: '#5c6573',
  brightRed: '#dc4d56',
  brightGreen: '#1d9460',
  brightYellow: '#b07a14',
  brightBlue: '#3b5bdb',
  brightMagenta: '#8f5cb8',
  brightCyan: '#1a8fa6',
  brightWhite: '#1c2430',
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
  return normalizeAppearance(appearance) === 'light' ? CONSOLE_TERM_LIGHT : CONSOLE_TERM_DARK
}
