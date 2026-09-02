import { Link, Outlet, useLocation, useNavigate } from 'react-router-dom'
import { useEffect, useMemo, useRef, useState } from 'react'
import { api, clearToken, getToken } from './api'
import { Icon } from './components/Icons'
import { applyTheme } from './theme'
import { useConfirm } from './components/Confirm'
import { useInventory } from './useInventory'
import ResourceTree from './components/ResourceTree'
import GuestWizard from './components/GuestWizard'
import ChangePassword from './components/ChangePassword'
import { parseResourceRoute, resourceLink } from './resourceRoutes'

const TREE_KEY = 'pertisk_vm_tree_collapsed'

export default function Layout() {
  const nav = useNavigate()
  const location = useLocation()
  const confirm = useConfirm()
  const inv = useInventory()
  const [user, setUser] = useState(null)
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') || 'dark')
  const [showUserMenu, setShowUserMenu] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem(TREE_KEY) === 'true')
  const [wizard, setWizard] = useState(false)
  const [passwordOpen, setPasswordOpen] = useState(false)
  const userMenuRef = useRef(null)

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
    localStorage.setItem(TREE_KEY, String(collapsed))
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
  const quorum = inv.cluster?.quorum !== false
  const currentRoute = useMemo(() => parseResourceRoute(location.pathname), [location.pathname])

  return (
    <div className="pve-shell">
      <header className="pve-header">
        <button
          type="button"
          className="pve-icon-btn pve-mobile-toggle"
          onClick={() => setMobileOpen((v) => !v)}
          aria-label="Toggle resource tree"
        >
          <Icon name="menu" size={18} />
        </button>
        <Link to={resourceLink('dc', null, currentRoute)} className="pve-brand">
          <span className="brand-mark" aria-hidden>
            <Icon name="guests" size={15} />
          </span>
          <span>
            Pertisk <span className="accent">VM</span>
          </span>
        </Link>
        <span className={`pve-quorum ${quorum ? 'ok' : 'bad'}`}>
          <Icon name={quorum ? 'check' : 'alert'} size={13} />
          {quorum ? 'Quorate' : 'No quorum'}
        </span>
        <div className="pve-header-spacer" />
        {canWrite && (
          <button type="button" className="pve-header-btn" onClick={() => setWizard(true)}>
            <Icon name="plus" size={15} />
            <span>Create guest</span>
          </button>
        )}
        <button
          type="button"
          className="pve-icon-btn"
          onClick={inv.refresh}
          title="Refresh"
          aria-label="Refresh"
        >
          <Icon name="refresh" size={16} />
        </button>
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
                  setShowUserMenu(false)
                  setPasswordOpen(true)
                }}
              >
                <Icon name="key" size={16} />
                Change password
              </button>
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

      <div className="pve-body">
        <div
          className={`sidebar-backdrop${mobileOpen ? ' open' : ''}`}
          aria-hidden={!mobileOpen}
          onClick={() => setMobileOpen(false)}
        />
        <aside className={`pve-tree${mobileOpen ? ' open' : ''}${collapsed ? ' collapsed' : ''}`}>
          <button
            type="button"
            className="pve-tree-collapse"
            onClick={() => setCollapsed((v) => !v)}
            title={collapsed ? 'Expand tree' : 'Collapse tree'}
            aria-label={collapsed ? 'Expand tree' : 'Collapse tree'}
          >
            <Icon name={collapsed ? 'chevrons-right' : 'chevrons-left'} size={14} />
          </button>
          <ResourceTree
            cluster={inv.cluster}
            host={inv.host}
            vms={inv.vms}
          />
        </aside>

        <main className="pve-main">
          <Outlet context={{ user, canWrite, inv }} />
        </main>
      </div>

      {passwordOpen && <ChangePassword onClose={() => setPasswordOpen(false)} />}

      {wizard && (
        <GuestWizard
          vms={inv.vms}
          volumes={inv.volumes}
          isos={inv.isos}
          networks={inv.networks}
          host={inv.host}
          cluster={inv.cluster}
          onClose={() => setWizard(false)}
          onCreated={async () => {
            await inv.refresh()
          }}
        />
      )}
    </div>
  )
}
