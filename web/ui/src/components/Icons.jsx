import {
  HiOutlineViewGrid,
  HiOutlineDesktopComputer,
  HiOutlineDatabase,
  HiOutlineGlobe,
  HiOutlineStatusOnline,
  HiOutlineClipboardList,
  HiOutlinePlus,
  HiOutlineTrash,
  HiOutlineSun,
  HiOutlineMoon,
  HiOutlineLogout,
  HiPlay,
  HiStop,
  HiOutlineChip,
  HiOutlineViewBoards,
  HiOutlineServer,
  HiOutlineExclamation,
  HiOutlineMenu,
  HiOutlineTerminal,
  HiOutlineUser,
  HiOutlineUsers,
  HiOutlineSwitchHorizontal,
  HiOutlineCheck,
  HiOutlineX,
  HiOutlineRefresh,
  HiOutlineCollection,
  HiOutlineChevronRight,
  HiOutlineChevronDown,
  HiOutlineChevronUp,
  HiOutlineFolder,
  HiOutlineOfficeBuilding,
  HiOutlineDocumentText,
  HiOutlineCog,
  HiOutlineClock,
  HiOutlineAdjustments,
  HiOutlineKey,
} from 'react-icons/hi'

const ICONS = {
  overview: HiOutlineViewGrid,
  guests: HiOutlineDesktopComputer,
  disk: HiOutlineDatabase,
  network: HiOutlineGlobe,
  cluster: HiOutlineStatusOnline,
  activity: HiOutlineClipboardList,
  plus: HiOutlinePlus,
  trash: HiOutlineTrash,
  sun: HiOutlineSun,
  moon: HiOutlineMoon,
  logout: HiOutlineLogout,
  play: HiPlay,
  stop: HiStop,
  cpu: HiOutlineChip,
  memory: HiOutlineViewBoards,
  worker: HiOutlineServer,
  alert: HiOutlineExclamation,
  check: HiOutlineCheck,
  x: HiOutlineX,
  menu: HiOutlineMenu,
  terminal: HiOutlineTerminal,
  user: HiOutlineUser,
  users: HiOutlineUsers,
  migrate: HiOutlineSwitchHorizontal,
  refresh: HiOutlineRefresh,
  volumes: HiOutlineCollection,
  'chevron-right': HiOutlineChevronRight,
  'chevron-down': HiOutlineChevronDown,
  'chevron-up': HiOutlineChevronUp,
  folder: HiOutlineFolder,
  datacenter: HiOutlineOfficeBuilding,
  summary: HiOutlineDocumentText,
  hardware: HiOutlineCog,
  clock: HiOutlineClock,
  options: HiOutlineAdjustments,
  key: HiOutlineKey,
}

export function Icon({ name, size = 18, className = '' }) {
  const Cmp = ICONS[name]
  if (!Cmp) return null
  return <Cmp size={size} className={`icon ${className}`.trim()} aria-hidden />
}

export function Btn({ icon, children, variant = 'primary', className = '', ...rest }) {
  const v = variant === 'primary' ? '' : variant
  return (
    <button type="button" className={`btn-icon ${v} ${className}`.trim()} {...rest}>
      {icon && <Icon name={icon} size={16} />}
      {children && <span>{children}</span>}
    </button>
  )
}
