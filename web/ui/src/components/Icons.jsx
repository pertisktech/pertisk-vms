import {
  LuLayoutGrid,
  LuMonitor,
  LuHardDrive,
  LuGlobe,
  LuNetwork,
  LuClipboardList,
  LuPlus,
  LuTrash2,
  LuSun,
  LuMoon,
  LuPalette,
  LuLogOut,
  LuPlay,
  LuPower,
  LuSquare,
  LuMonitorPlay,
  LuCpu,
  LuMemoryStick,
  LuServer,
  LuTriangleAlert,
  LuMenu,
  LuSquareTerminal,
  LuUser,
  LuUsers,
  LuArrowLeftRight,
  LuCheck,
  LuX,
  LuRefreshCw,
  LuLayers,
  LuChevronLeft,
  LuChevronRight,
  LuChevronDown,
  LuChevronUp,
  LuFolder,
  LuBuilding2,
  LuFileText,
  LuSettings,
  LuClock,
  LuSlidersHorizontal,
  LuKeyRound,
  LuCopy,
  LuLayoutTemplate,
  LuDownload,
  LuLibrary,
} from 'react-icons/lu'

const ICONS = {
  overview: LuLayoutGrid,
  guests: LuMonitor,
  disk: LuHardDrive,
  network: LuGlobe,
  cluster: LuNetwork,
  activity: LuClipboardList,
  plus: LuPlus,
  trash: LuTrash2,
  sun: LuSun,
  moon: LuMoon,
  palette: LuPalette,
  logout: LuLogOut,
  play: LuPlay,
  power: LuPower,
  stop: LuSquare,
  console: LuMonitorPlay,
  cpu: LuCpu,
  memory: LuMemoryStick,
  worker: LuServer,
  alert: LuTriangleAlert,
  check: LuCheck,
  x: LuX,
  menu: LuMenu,
  terminal: LuSquareTerminal,
  user: LuUser,
  users: LuUsers,
  migrate: LuArrowLeftRight,
  refresh: LuRefreshCw,
  volumes: LuLayers,
  'chevron-left': LuChevronLeft,
  'chevron-right': LuChevronRight,
  'chevron-down': LuChevronDown,
  'chevron-up': LuChevronUp,
  folder: LuFolder,
  datacenter: LuBuilding2,
  summary: LuFileText,
  hardware: LuSettings,
  clock: LuClock,
  options: LuSlidersHorizontal,
  key: LuKeyRound,
  clone: LuCopy,
  template: LuLayoutTemplate,
  updates: LuDownload,
  repo: LuLibrary,
}

export function Icon({ name, size = 18, className = '' }) {
  const Cmp = ICONS[name]
  if (!Cmp) return null
  return <Cmp size={size} strokeWidth={1.75} className={`icon ${className}`.trim()} aria-hidden />
}

export function Btn({ icon, children, variant = 'primary', className = '', onMouseDown, ...rest }) {
  const v = variant === 'primary' ? '' : variant
  return (
    <button
      type="button"
      className={`btn-icon ${v} ${className}`.trim()}
      // Keep console/VNC focus: mousedown focus-steal makes Enter hit Restart/Refresh.
      onMouseDown={(e) => {
        e.preventDefault()
        onMouseDown?.(e)
      }}
      {...rest}
    >
      {icon && <Icon name={icon} size={16} />}
      {children && <span>{children}</span>}
    </button>
  )
}
