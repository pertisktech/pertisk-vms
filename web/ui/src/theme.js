export const THEME_PRESET_KEY = 'pertisk_theme_preset'
export const THEME_APPEARANCE_KEY = 'pertisk_theme_appearance'

const CONSOLE_TERM_DARK = {
  background: '#17132a',
  foreground: '#e7e1f5',
  cursor: '#c4b5fd',
  cursorAccent: '#17132a',
  selectionBackground: '#4c3f7a',
  selectionInactiveBackground: '#322a52',
  black: '#1e1a30',
  red: '#ff7b72',
  green: '#8ce39b',
  yellow: '#f0d98c',
  blue: '#a78bfa',
  magenta: '#c792ea',
  cyan: '#89ddff',
  white: '#d4cce8',
  brightBlack: '#6b6390',
  brightRed: '#ff8a84',
  brightGreen: '#7aecb8',
  brightYellow: '#fcd34d',
  brightBlue: '#c4b5fd',
  brightMagenta: '#e0c4ff',
  brightCyan: '#a5f3fc',
  brightWhite: '#ffffff',
}

const CONSOLE_TERM_LIGHT = {
  background: '#f6f2fb',
  foreground: '#2c2542',
  cursor: '#6d4ad4',
  cursorAccent: '#f6f2fb',
  selectionBackground: '#ddd4f5',
  selectionInactiveBackground: '#ece7f6',
  black: '#2c2542',
  red: '#c23d48',
  green: '#217a4c',
  yellow: '#8f6410',
  blue: '#5340c5',
  magenta: '#933a9e',
  cyan: '#147a8e',
  white: '#d8d2e6',
  brightBlack: '#6e668c',
  brightRed: '#dc4d56',
  brightGreen: '#1d9460',
  brightYellow: '#b07a14',
  brightBlue: '#6b57db',
  brightMagenta: '#b04ab8',
  brightCyan: '#1a8fa6',
  brightWhite: '#1f1a32',
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
