import { NavLink, Outlet, useLocation, useNavigate } from 'react-router-dom'
import { api, clearToken, getToken } from './api'
import { useEffect, useRef, useState } from 'react'
import { Icon } from './components/Icons'
import { applyTheme } from './theme'
import { useConfirm } from './components/Confirm'

const SIDEBAR_KEY = 'pertisk_vm_sidebar_collapsed'

const NAV = [
  { to: '/', label: 'Overview', icon: 'overview', end: true },
  { to: '/guests', label: 'Guests', icon: 'guests' },
  { to: '/storage', label: 'Storage', icon: 'disk' },
  { to: '/networks', label: 'Networks', icon: 'network' },
  { to: '/cluster', label: 'Cluster', icon: 'cluster' },
  { to: '/activity', label: 'Activity', icon: 'activity' },
  { to: '/users', label: 'Users', icon: 'users', adminOnly: true },
]

function resolveTitle(pathname) {
  const match = NAV.filter((n) =>
    n.end ? pathname === n.to : pathname === n.to || pathname.startsWith(`${n.to}/`),
  ).sort((a, b) => b.to.length - a.to.length)[0]
  return match?.label ?? 'Pertisk VM'
}

export default function Layout() {
  const nav = useNavigate()
  const location = useLocation()
  const confirm = useConfirm()
  const [user, setUser] = useState(null)
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') || 'dark')
  const [showUserMenu, setShowUserMenu] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem(SIDEBAR_KEY) === 'true')
  const userMenuRef = useRef(null)
  const title = resolveTitle(location.pathname)

  useEffect(() => {
    applyTheme(theme)
  }, [theme])

  useEffect(() => {
    if (!getToken()) {
      nav('/login')
      return
    }
    api('/v1/session')
      .then(setUser)
      .catch(() => {
        clearToken()
        nav('/login')
      })
  }, [nav])

  useEffect(() => {
    setMobileOpen(false)
    setShowUserMenu(false)
  }, [location.pathname])

  useEffect(() => {
    localStorage.setItem(SIDEBAR_KEY, String(collapsed))
  }, [collapsed])

  useEffect(() => {
    if (!showUserMenu) return
    function onPointerDown(e) {
      if (userMenuRef.current && !userMenuRef.current.contains(e.target)) {
        setShowUserMenu(false)
      }
    }
    document.addEventListener('pointerdown', onPointerDown)
    return () => document.removeEventListener('pointerdown', onPointerDown)
  }, [showUserMenu])

  async function logout() {
    setShowUserMenu(false)
    const ok = await confirm({
      title: 'Sign out',
      message: 'End your session on this device?',
      confirmLabel: 'Sign out',
      tone: 'primary',
    })
    if (!ok) return
    clearToken()
    nav('/login')
  }

  const initial = user?.username ? user.username.charAt(0).toUpperCase() : 'U'
  const canWrite = user?.role && user.role !== 'viewer'

  return (
    <div className="shell">
      <div
        className={`sidebar-backdrop${mobileOpen ? ' open' : ''}`}
        aria-hidden={!mobileOpen}
        onClick={() => setMobileOpen(false)}
      />
      <aside className={`sidebar${mobileOpen ? ' open' : ''}${collapsed ? ' collapsed' : ''}`}>
        <div className="sidebar-header">
          <div className="brand">
            <span className="brand-mark" aria-hidden>
              <Icon name="guests" size={16} />
            </span>
            <div className="brand-text">
              <span>
                Pertisk <span className="accent">VM</span>
              </span>
            </div>
          </div>
          <button
            type="button"
            className={`sidebar-collapse-btn${!collapsed ? ' anchor-right' : ''}`}
            onClick={() => setCollapsed((v) => !v)}
            title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            <Icon name={collapsed ? 'chevrons-right' : 'chevrons-left'} size={16} />
          </button>
        </div>
        <nav className="nav" aria-label="Primary">
          {NAV.filter((n) => !n.adminOnly || user?.role === 'admin').map(({ to, label, icon, end }) => (
            <NavLink key={to} to={to} end={end} className={({ isActive }) => (isActive ? 'active' : '')}>
              <Icon name={icon} className="icon" size={18} />
              <span className="nav-label">{label}</span>
            </NavLink>
          ))}
        </nav>
      </aside>

      <div className={`main${mobileOpen ? ' sidebar-open' : ''}`}>
        <header className="topbar">
          <div className="topbar-left">
            <button
              type="button"
              className="secondary topbar-menu-btn"
              onClick={() => setMobileOpen(true)}
              aria-label="Open menu"
            >
              <Icon name="menu" size={18} />
            </button>
            <h1 className="topbar-title">{title}</h1>
          </div>
          <div className="user-menu" ref={userMenuRef}>
            <button
              type="button"
              className={`user-menu-trigger${showUserMenu ? ' open' : ''}`}
              onClick={() => setShowUserMenu((v) => !v)}
            >
              <span className="user-avatar">{initial}</span>
              <span className="user-name">{user?.username || '…'}</span>
            </button>
            {showUserMenu && (
              <div className="user-menu-dropdown">
                <div className="user-menu-meta">{user?.role || 'session'}</div>
                <button
                  type="button"
                  onClick={() => {
                    const next = theme === 'dark' ? 'light' : 'dark'
                    setTheme(next)
                    applyTheme(next)
                  }}
                >
                  <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={16} />
                  {theme === 'dark' ? 'Light' : 'Dark'}
                </button>
                <button type="button" onClick={logout}>
                  <Icon name="logout" size={16} />
                  Sign out
                </button>
              </div>
            )}
          </div>
        </header>
        <main className="content">
          <Outlet context={{ user, canWrite }} />
        </main>
      </div>
    </div>
  )
}
