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

const CONSOLE_TERM_LIGHT = {
  background: '#ffffff',
  foreground: '#2c2340',
  cursor: '#7c3aed',
  cursorAccent: '#ffffff',
  selectionBackground: '#e3d8fb',
  black: '#2c2340',
  red: '#c0392b',
  green: '#2f8f4e',
  yellow: '#9a7d20',
  blue: '#6d28d9',
  magenta: '#8b5cf6',
  cyan: '#0e7490',
  white: '#2c2340',
  brightBlack: '#8a80a8',
  brightRed: '#e24b3d',
  brightGreen: '#3cb86a',
  brightYellow: '#c9a227',
  brightBlue: '#7c3aed',
  brightMagenta: '#a78bfa',
  brightCyan: '#22d3ee',
  brightWhite: '#1a1626',
}

export const APP_THEME_PRESETS = [
  {
    id: 'console-violet',
    label: 'Console Violet',
    description: 'Proxmox-style purple console palette with light and dark modes.',
    tokens: {},
  },
  {
    id: 'flux-violet',
    label: 'Flux Violet',
    description: 'The existing violet desktop palette used across the app.',
    tokens: {
      '--color-primary-p1': '#d8c8fd',
      '--color-primary-p2': '#b89ef9',
      '--color-primary-p3': '#9a7bf7',
      '--color-primary-p4': '#7c59f0',
      '--color-primary-p5': '#5f3fd4',
      '--color-primary-p6': '#281a5e',
      '--color-primary': '#9a7bf7',
      '--color-primary-hover': '#b89ef9',
      '--color-sidebar': '#090a14',
      '--color-bg': '#0c0d18',
      '--color-surface': '#10111e',
      '--color-surface-elevated': '#131421',
      '--color-hover': '#1d1f32',
      '--color-border': '#23253c',
      '--color-card': '#131421',
      '--color-bg-gradient-start': '#0c0d18',
      '--color-bg-gradient-end': '#10111e',
      '--color-icon-primary': '#9a7bf7',
      '--color-dashboard-metric-secondary': '#a855f7',
      '--color-dashboard-metric-secondary-bg': 'rgba(168, 85, 247, 0.12)',
      '--color-workload-accent': '#14b8a6',
      '--color-workload-accent-strong': '#0f766e',
    },
  },
  {
    id: 'ocean-blue',
    label: 'Ocean Blue',
    description: 'Cool cobalt accents with a brighter, terminal-like blue edge.',
    tokens: {
      '--color-primary-p1': '#cbe4ff',
      '--color-primary-p2': '#9fcbff',
      '--color-primary-p3': '#71b3ff',
      '--color-primary-p4': '#3e8fff',
      '--color-primary-p5': '#1b68d8',
      '--color-primary-p6': '#10274f',
      '--color-primary': '#71b3ff',
      '--color-primary-hover': '#9fcbff',
      '--color-sidebar': '#08111d',
      '--color-bg': '#08111d',
      '--color-surface': '#0d1728',
      '--color-surface-elevated': '#111c30',
      '--color-hover': '#162236',
      '--color-border': '#1d2b42',
      '--color-card': '#111c30',
      '--color-bg-gradient-start': '#08111d',
      '--color-bg-gradient-end': '#0d1728',
      '--color-icon-primary': '#71b3ff',
      '--color-dashboard-metric-secondary': '#38bdf8',
      '--color-dashboard-metric-secondary-bg': 'rgba(56, 189, 248, 0.12)',
      '--color-workload-accent': '#0ea5e9',
      '--color-workload-accent-strong': '#0369a1',
    },
  },
  {
    id: 'forest-teal',
    label: 'Forest Teal',
    description: 'A greener preset with softer mint and teal emphasis.',
    tokens: {
      '--color-primary-p1': '#c9f2e1',
      '--color-primary-p2': '#95e0c1',
      '--color-primary-p3': '#5fcaa2',
      '--color-primary-p4': '#2ca17d',
      '--color-primary-p5': '#14634d',
      '--color-primary-p6': '#0b2e24',
      '--color-primary': '#5fcaa2',
      '--color-primary-hover': '#95e0c1',
      '--color-sidebar': '#07120f',
      '--color-bg': '#091512',
      '--color-surface': '#0d1c17',
      '--color-surface-elevated': '#11231c',
      '--color-hover': '#162b22',
      '--color-border': '#1e3428',
      '--color-card': '#11231c',
      '--color-bg-gradient-start': '#091512',
      '--color-bg-gradient-end': '#0d1c17',
      '--color-icon-primary': '#5fcaa2',
      '--color-dashboard-metric-secondary': '#14b8a6',
      '--color-dashboard-metric-secondary-bg': 'rgba(20, 184, 166, 0.12)',
      '--color-workload-accent': '#34d399',
      '--color-workload-accent-strong': '#047857',
    },
  },
  {
    id: 'ember-rose',
    label: 'Ember Rose',
    description: 'Warm red and amber accents for a higher-contrast color story.',
    tokens: {
      '--color-primary-p1': '#ffd5df',
      '--color-primary-p2': '#ffabc0',
      '--color-primary-p3': '#ff7b9c',
      '--color-primary-p4': '#ef5476',
      '--color-primary-p5': '#b9354f',
      '--color-primary-p6': '#4a1622',
      '--color-primary': '#ff7b9c',
      '--color-primary-hover': '#ffabc0',
      '--color-sidebar': '#14090b',
      '--color-bg': '#130c11',
      '--color-surface': '#1b1016',
      '--color-surface-elevated': '#221419',
      '--color-hover': '#2a181e',
      '--color-border': '#3a2028',
      '--color-card': '#221419',
      '--color-bg-gradient-start': '#130c11',
      '--color-bg-gradient-end': '#1b1016',
      '--color-icon-primary': '#ff7b9c',
      '--color-dashboard-metric-secondary': '#fb7185',
      '--color-dashboard-metric-secondary-bg': 'rgba(251, 113, 133, 0.14)',
      '--color-workload-accent': '#f59e0b',
      '--color-workload-accent-strong': '#b45309',
    },
  },
  {
    id: 'wild-cherry',
    label: 'Wild Cherry',
    description: 'Canonical Wild Cherry palette mapped from the iTerm2 scheme.',
    tokens: {
      '--color-primary-p1': '#E4FFFF',
      '--color-primary-p2': '#DA6BAC',
      '--color-primary-p3': '#D94085',
      '--color-primary-p4': '#DD00FF',
      '--color-primary-p5': '#AE636B',
      '--color-primary-p6': '#000507',
      '--color-primary': '#D94085',
      '--color-primary-hover': '#DA6BAC',
      '--color-sidebar': '#160F1B',
      '--color-bg': '#1F1726',
      '--color-surface': '#251C2D',
      '--color-surface-elevated': '#2B2235',
      '--color-hover': '#002831',
      '--color-border': '#5B3A56',
      '--color-card': '#2B2235',
      '--color-text': '#DAFAFF',
      '--color-text-secondary': '#C1B8B7',
      '--color-muted': '#AE636B',
      '--color-bg-gradient-start': '#1F1726',
      '--color-bg-gradient-end': '#2B2235',
      '--color-icon-primary': '#D94085',
      '--color-dashboard-metric-secondary': '#FF919D',
      '--color-dashboard-metric-secondary-bg': 'rgba(255, 145, 157, 0.16)',
      '--color-workload-accent': '#009CC9',
      '--color-workload-accent-strong': '#308CBA',
    },
  },
  {
    id: 'midnight-cyan',
    label: 'Midnight Cyan',
    description: 'Deep midnight tones with electric cyan accents.',
    tokens: {
      '--color-primary-p1': '#b8f4f8',
      '--color-primary-p2': '#7aeef4',
      '--color-primary-p3': '#22d3ee',
      '--color-primary-p4': '#06b6d4',
      '--color-primary-p5': '#0891b2',
      '--color-primary-p6': '#164e63',
      '--color-primary': '#22d3ee',
      '--color-primary-hover': '#7aeef4',
      '--color-sidebar': '#0a0e14',
      '--color-bg': '#0c1218',
      '--color-surface': '#121a22',
      '--color-surface-elevated': '#18222c',
      '--color-hover': '#1e2a36',
      '--color-border': '#263442',
      '--color-card': '#18222c',
      '--color-bg-gradient-start': '#0c1218',
      '--color-bg-gradient-end': '#121a22',
      '--color-icon-primary': '#22d3ee',
      '--color-dashboard-metric-secondary': '#06b6d4',
      '--color-dashboard-metric-secondary-bg': 'rgba(6, 182, 212, 0.12)',
      '--color-workload-accent': '#14b8a6',
      '--color-workload-accent-strong': '#0f766e',
    },
  },
  {
    id: 'sunset-amber',
    label: 'Sunset Amber',
    description: 'Warm amber and orange tones evoking a sunset glow.',
    tokens: {
      '--color-primary-p1': '#fef3c7',
      '--color-primary-p2': '#fde68a',
      '--color-primary-p3': '#fbbf24',
      '--color-primary-p4': '#f59e0b',
      '--color-primary-p5': '#d97706',
      '--color-primary-p6': '#78350f',
      '--color-primary': '#fbbf24',
      '--color-primary-hover': '#fde68a',
      '--color-sidebar': '#14100a',
      '--color-bg': '#18140e',
      '--color-surface': '#1e1a12',
      '--color-surface-elevated': '#252018',
      '--color-hover': '#2c261c',
      '--color-border': '#3d3424',
      '--color-card': '#252018',
      '--color-bg-gradient-start': '#18140e',
      '--color-bg-gradient-end': '#1e1a12',
      '--color-icon-primary': '#fbbf24',
      '--color-dashboard-metric-secondary': '#f59e0b',
      '--color-dashboard-metric-secondary-bg': 'rgba(245, 158, 11, 0.12)',
      '--color-workload-accent': '#fb923c',
      '--color-workload-accent-strong': '#ea580c',
    },
  },
  {
    id: 'arctic-frost',
    label: 'Arctic Frost',
    description: 'Cool icy blues with a frosty, modern aesthetic.',
    tokens: {
      '--color-primary-p1': '#e0f2fe',
      '--color-primary-p2': '#bae6fd',
      '--color-primary-p3': '#7dd3fc',
      '--color-primary-p4': '#38bdf8',
      '--color-primary-p5': '#0ea5e9',
      '--color-primary-p6': '#075985',
      '--color-primary': '#7dd3fc',
      '--color-primary-hover': '#bae6fd',
      '--color-sidebar': '#0a1018',
      '--color-bg': '#0c141c',
      '--color-surface': '#121c28',
      '--color-surface-elevated': '#182432',
      '--color-hover': '#1e2c3c',
      '--color-border': '#28384a',
      '--color-card': '#182432',
      '--color-bg-gradient-start': '#0c141c',
      '--color-bg-gradient-end': '#121c28',
      '--color-icon-primary': '#7dd3fc',
      '--color-dashboard-metric-secondary': '#38bdf8',
      '--color-dashboard-metric-secondary-bg': 'rgba(56, 189, 248, 0.12)',
      '--color-workload-accent': '#06b6d4',
      '--color-workload-accent-strong': '#0891b2',
    },
  },
  {
    id: 'neon-pink',
    label: 'Neon Pink',
    description: 'Vibrant neon pink with a cyberpunk aesthetic.',
    tokens: {
      '--color-primary-p1': '#fce7f3',
      '--color-primary-p2': '#fbcfe8',
      '--color-primary-p3': '#f472b6',
      '--color-primary-p4': '#ec4899',
      '--color-primary-p5': '#db2777',
      '--color-primary-p6': '#831843',
      '--color-primary': '#f472b6',
      '--color-primary-hover': '#fbcfe8',
      '--color-sidebar': '#12080e',
      '--color-bg': '#160a12',
      '--color-surface': '#1e1018',
      '--color-surface-elevated': '#26141e',
      '--color-hover': '#2e1824',
      '--color-border': '#3e2030',
      '--color-card': '#26141e',
      '--color-bg-gradient-start': '#160a12',
      '--color-bg-gradient-end': '#1e1018',
      '--color-icon-primary': '#f472b6',
      '--color-dashboard-metric-secondary': '#ec4899',
      '--color-dashboard-metric-secondary-bg': 'rgba(236, 72, 153, 0.12)',
      '--color-workload-accent': '#a855f7',
      '--color-workload-accent-strong': '#7c3aed',
    },
  },
  {
    id: 'olive-dusk',
    label: 'Olive Dusk',
    description: 'Earthy olive and sage tones for a natural feel.',
    tokens: {
      '--color-primary-p1': '#ecfccb',
      '--color-primary-p2': '#d9f99d',
      '--color-primary-p3': '#bef264',
      '--color-primary-p4': '#a3e635',
      '--color-primary-p5': '#84cc16',
      '--color-primary-p6': '#365314',
      '--color-primary': '#a3e635',
      '--color-primary-hover': '#bef264',
      '--color-sidebar': '#0e100a',
      '--color-bg': '#12140e',
      '--color-surface': '#181a14',
      '--color-surface-elevated': '#1e2118',
      '--color-hover': '#24281c',
      '--color-border': '#323824',
      '--color-card': '#1e2118',
      '--color-bg-gradient-start': '#12140e',
      '--color-bg-gradient-end': '#181a14',
      '--color-icon-primary': '#a3e635',
      '--color-dashboard-metric-secondary': '#84cc16',
      '--color-dashboard-metric-secondary-bg': 'rgba(132, 204, 22, 0.12)',
      '--color-workload-accent': '#22c55e',
      '--color-workload-accent-strong': '#16a34a',
    },
  },
  {
    id: 'slate-gray',
    label: 'Slate Gray',
    description: 'Neutral slate tones for a minimal, professional look.',
    tokens: {
      '--color-primary-p1': '#e2e8f0',
      '--color-primary-p2': '#cbd5e1',
      '--color-primary-p3': '#94a3b8',
      '--color-primary-p4': '#64748b',
      '--color-primary-p5': '#475569',
      '--color-primary-p6': '#1e293b',
      '--color-primary': '#94a3b8',
      '--color-primary-hover': '#cbd5e1',
      '--color-sidebar': '#0c0e12',
      '--color-bg': '#0f1216',
      '--color-surface': '#15181e',
      '--color-surface-elevated': '#1a1e26',
      '--color-hover': '#20252e',
      '--color-border': '#2a303c',
      '--color-card': '#1a1e26',
      '--color-bg-gradient-start': '#0f1216',
      '--color-bg-gradient-end': '#15181e',
      '--color-icon-primary': '#94a3b8',
      '--color-dashboard-metric-secondary': '#64748b',
      '--color-dashboard-metric-secondary-bg': 'rgba(100, 116, 139, 0.12)',
      '--color-workload-accent': '#38bdf8',
      '--color-workload-accent-strong': '#0ea5e9',
    },
  },
  {
    id: 'golden-hour',
    label: 'Golden Hour',
    description: 'Soft golden yellows with warm, inviting tones.',
    tokens: {
      '--color-primary-p1': '#fef9c3',
      '--color-primary-p2': '#fef08a',
      '--color-primary-p3': '#facc15',
      '--color-primary-p4': '#eab308',
      '--color-primary-p5': '#ca8a04',
      '--color-primary-p6': '#713f12',
      '--color-primary': '#facc15',
      '--color-primary-hover': '#fef08a',
      '--color-sidebar': '#14120a',
      '--color-bg': '#18160c',
      '--color-surface': '#1e1c12',
      '--color-surface-elevated': '#262318',
      '--color-hover': '#2e2a1c',
      '--color-border': '#3e3824',
      '--color-card': '#262318',
      '--color-bg-gradient-start': '#18160c',
      '--color-bg-gradient-end': '#1e1c12',
      '--color-icon-primary': '#facc15',
      '--color-dashboard-metric-secondary': '#eab308',
      '--color-dashboard-metric-secondary-bg': 'rgba(234, 179, 8, 0.12)',
      '--color-workload-accent': '#f59e0b',
      '--color-workload-accent-strong': '#d97706',
    },
  },
  {
    id: 'deep-purple',
    label: 'Deep Purple',
    description: 'Rich purple tones with a luxurious, deep aesthetic.',
    tokens: {
      '--color-primary-p1': '#ede9fe',
      '--color-primary-p2': '#ddd6fe',
      '--color-primary-p3': '#a78bfa',
      '--color-primary-p4': '#8b5cf6',
      '--color-primary-p5': '#7c3aed',
      '--color-primary-p6': '#4c1d95',
      '--color-primary': '#a78bfa',
      '--color-primary-hover': '#ddd6fe',
      '--color-sidebar': '#0e0a14',
      '--color-bg': '#120e18',
      '--color-surface': '#18141e',
      '--color-surface-elevated': '#1e1826',
      '--color-hover': '#241e2e',
      '--color-border': '#32283e',
      '--color-card': '#1e1826',
      '--color-bg-gradient-start': '#120e18',
      '--color-bg-gradient-end': '#18141e',
      '--color-icon-primary': '#a78bfa',
      '--color-dashboard-metric-secondary': '#8b5cf6',
      '--color-dashboard-metric-secondary-bg': 'rgba(139, 92, 246, 0.12)',
      '--color-workload-accent': '#c084fc',
      '--color-workload-accent-strong': '#a855f7',
    },
  },
]

const THEME_TOKEN_KEYS = [...new Set(APP_THEME_PRESETS.flatMap((preset) => Object.keys(preset.tokens)))]

export function normalizeThemePreset(value) {
  return APP_THEME_PRESETS.some((preset) => preset.id === value) ? value : 'console-violet'
}

export function getStoredThemePreset() {
  try {
    return normalizeThemePreset(localStorage.getItem(THEME_PRESET_KEY))
  } catch {
    return 'console-violet'
  }
}

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

export function applyAppearance(appearance) {
  const mode = normalizeAppearance(appearance)
  const root = document.documentElement
  root.classList.toggle('dark', mode === 'dark')
  root.classList.toggle('light', mode === 'light')
  root.style.colorScheme = mode
  try {
    localStorage.setItem(THEME_APPEARANCE_KEY, mode)
  } catch {
    /* ignore quota / private mode */
  }
  return mode
}

export function applyThemePreset(presetId) {
  const id = normalizeThemePreset(presetId)
  const preset = APP_THEME_PRESETS.find((item) => item.id === id) || APP_THEME_PRESETS[0]
  const root = document.documentElement
  root.removeAttribute('data-theme')
  for (const key of THEME_TOKEN_KEYS) {
    root.style.removeProperty(key)
  }
  Object.entries(preset.tokens).forEach(([token, value]) => {
    root.style.setProperty(token, value)
  })
  try {
    localStorage.setItem(THEME_PRESET_KEY, id)
    localStorage.removeItem('theme')
  } catch {
    /* ignore quota / private mode */
  }
  return id
}

export function initTheme() {
  applyAppearance(getStoredAppearance())
  applyThemePreset(getStoredThemePreset())
}

export function terminalPalette(presetId, appearance) {
  const preset = APP_THEME_PRESETS.find((item) => item.id === presetId) || APP_THEME_PRESETS[0]
  const tokens = preset.tokens
  const mode = normalizeAppearance(appearance)
  if (!tokens['--color-bg']) {
    return mode === 'light' ? CONSOLE_TERM_LIGHT : CONSOLE_TERM_DARK
  }
  return {
    background: tokens['--color-bg'] || '#0c0d18',
    foreground: tokens['--color-text'] || '#e6e7f0',
    cursor: tokens['--color-primary'] || '#9a7bf7',
    cursorAccent: tokens['--color-bg'] || '#0c0d18',
    selectionBackground: tokens['--color-hover'] || '#1d1f32',
    black: tokens['--color-sidebar'] || '#090a14',
    red: '#ff5c56',
    green: '#4ade9b',
    yellow: '#fbbf24',
    blue: tokens['--color-primary-p3'] || '#71b3ff',
    magenta: tokens['--color-primary'] || '#9a7bf7',
    cyan: tokens['--color-workload-accent'] || '#14b8a6',
    white: '#e6e7f0',
    brightBlack: tokens['--color-border'] || '#23253c',
    brightRed: '#ff8a84',
    brightGreen: '#7aecb8',
    brightYellow: '#fcd34d',
    brightBlue: tokens['--color-primary-p2'] || '#9fcbff',
    brightMagenta: tokens['--color-primary-hover'] || '#b89ef9',
    brightCyan: '#5eead4',
    brightWhite: '#ffffff',
  }
}
