import { Link, Outlet, useLocation, useNavigate } from 'react-router-dom'
import { useEffect, useMemo, useRef, useState } from 'react'
import { api, asList, clearToken, getToken, isTemplate, onAuthRequired, setToken, tokenIsRemembered, clearAuthRequired, vmCaption } from './api'
import { Icon } from './components/Icons'
import { useTheme } from './ThemeContext'
import { useConfirm } from './components/Confirm'
import { useInventory } from './useInventory'
import ResourceTree from './components/ResourceTree'
import GuestWizard from './components/GuestWizard'
import ChangePassword from './components/ChangePassword'
import Modal from './components/Modal'
import { reconnectLive } from './live'
import { parseResourceRoute, resourceLink } from './resourceRoutes'

const TREE_KEY = 'pertisk_vm_tree_collapsed'
const TREE_WIDTH_KEY = 'pertisk_vm_tree_width'
const TREE_WIDTH_MIN = 180
const TREE_WIDTH_MAX = 560
const TREE_WIDTH_DEFAULT = 288

function clampTreeWidth(value) {
  const n = Number(value)
  if (!Number.isFinite(n)) return TREE_WIDTH_DEFAULT
  return Math.min(TREE_WIDTH_MAX, Math.max(TREE_WIDTH_MIN, Math.round(n)))
}

function ResourceSearch({ cluster, host, vms }) {
  const nav = useNavigate()
  const location = useLocation()
  const wrapRef = useRef(null)
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const currentRoute = useMemo(() => parseResourceRoute(location.pathname), [location.pathname])

  const results = useMemo(() => {
    const query = q.trim().toLowerCase()
    if (!query) return []
    const members = asList(cluster?.members)
    const nodes = members.length
      ? members
      : [{ id: 'local', name: host?.hostname || 'localhost' }]
    const out = []
    if ('datacenter'.includes(query) || 'pertisk'.includes(query)) {
      out.push({ to: resourceLink('dc', null, currentRoute), label: 'Datacenter', icon: 'datacenter' })
    }
    for (const node of nodes) {
      if (String(node.name || '').toLowerCase().includes(query) || String(node.id || '').toLowerCase().includes(query)) {
        out.push({ to: resourceLink('node', node.id, currentRoute), label: node.name, icon: 'worker' })
      }
    }
    for (const vm of vms) {
      const caption = vmCaption(vm)
      const hay = `${caption.id} ${caption.name || ''} ${caption.title}`.toLowerCase()
      if (hay.includes(query)) {
        out.push({
          to: resourceLink('vm', vm.id, currentRoute, { template: isTemplate(vm) }),
          label: caption.title,
          icon: isTemplate(vm) ? 'template' : 'guests',
        })
      }
    }
    return out.slice(0, 8)
  }, [q, cluster, host, vms, currentRoute])

  useEffect(() => {
    function onPointerDown(e) {
      if (wrapRef.current && !wrapRef.current.contains(e.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    return () => document.removeEventListener('pointerdown', onPointerDown)
  }, [])

  function go(to) {
    setQ('')
    setOpen(false)
    nav(to)
  }

  return (
    <div className="pve-search" ref={wrapRef}>
      <Icon name="search" size={16} />
      <input
        type="search"
        placeholder="Search resources…"
        value={q}
        onChange={(e) => {
          setQ(e.target.value)
          setOpen(true)
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && results[0]) {
            e.preventDefault()
            go(results[0].to)
          }
          if (e.key === 'Escape') setOpen(false)
        }}
      />
      {open && q.trim() && (
        <div className="pve-search-results">
          {results.length === 0 ? (
            <div className="pve-search-empty">No matches</div>
          ) : (
            results.map((item) => (
              <a
                key={item.to}
                href={`#${item.to}`}
                onClick={(e) => {
                  e.preventDefault()
                  go(item.to)
                }}
              >
                <Icon name={item.icon} size={14} />
                {item.label}
              </a>
            ))
          )}
        </div>
      )}
    </div>
  )
}

export default function Layout() {
  const nav = useNavigate()
  const location = useLocation()
  const confirm = useConfirm()
  const inv = useInventory()
  const { appearance, toggleAppearance } = useTheme()
  const [user, setUser] = useState(null)
  const [showUserMenu, setShowUserMenu] = useState(false)
  const [mobileOpen, setMobileOpen] = useState(false)
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem(TREE_KEY) === 'true')
  const [treeWidth, setTreeWidth] = useState(() => clampTreeWidth(localStorage.getItem(TREE_WIDTH_KEY)))
  const [resizing, setResizing] = useState(false)
  const [wizard, setWizard] = useState(false)
  const [passwordOpen, setPasswordOpen] = useState(false)
  const [reauth, setReauth] = useState(false)
  const [reauthUser, setReauthUser] = useState('admin')
  const [reauthPass, setReauthPass] = useState('')
  const [reauthError, setReauthError] = useState('')
  const [reauthBusy, setReauthBusy] = useState(false)
  const userMenuRef = useRef(null)
  const treeRef = useRef(null)

  useEffect(() => {
    if (!getToken()) {
      nav('/login')
      return
    }
    api('/v1/session')
      .then(setUser)
      .catch(() => {
        /* Keep the UI. Expired tokens open the re-auth dialog via onAuthRequired. */
      })
  }, [nav])

  useEffect(() => onAuthRequired(() => setReauth(true)), [])

  useEffect(() => {
    setMobileOpen(false)
    setShowUserMenu(false)
  }, [location.pathname])

  useEffect(() => {
    localStorage.setItem(TREE_KEY, String(collapsed))
  }, [collapsed])

  useEffect(() => {
    localStorage.setItem(TREE_WIDTH_KEY, String(treeWidth))
  }, [treeWidth])

  function toggleSidebar() {
    if (typeof window !== 'undefined' && window.matchMedia('(min-width: 1024px)').matches) {
      setCollapsed((v) => !v)
    } else {
      setMobileOpen((v) => !v)
    }
  }

  function onResizePointerDown(e) {
    if (e.button !== 0) return
    e.preventDefault()
    const startX = e.clientX
    const startWidth = treeRef.current?.getBoundingClientRect().width || treeWidth
    setResizing(true)
    const prevCursor = document.body.style.cursor
    const prevSelect = document.body.style.userSelect
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'

    function onMove(ev) {
      setTreeWidth(clampTreeWidth(startWidth + (ev.clientX - startX)))
    }
    function onUp() {
      setResizing(false)
      document.body.style.cursor = prevCursor
      document.body.style.userSelect = prevSelect
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

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
    clearAuthRequired()
    clearToken()
    nav('/login')
  }

  function signOutExpired() {
    clearAuthRequired()
    clearToken()
    setReauth(false)
    nav('/login')
  }

  async function reauthSubmit(e) {
    e.preventDefault()
    setReauthBusy(true)
    setReauthError('')
    try {
      const res = await api('/v1/login', {
        method: 'POST',
        body: { username: reauthUser.trim(), password: reauthPass },
      })
      setToken(res.token, tokenIsRemembered())
      clearAuthRequired()
      const session = await api('/v1/session')
      setUser(session)
      setReauthPass('')
      setReauth(false)
      reconnectLive()
      await inv.refresh()
    } catch (err) {
      setReauthError(err.message || String(err))
    } finally {
      setReauthBusy(false)
    }
  }

  const initial = user?.username ? user.username.charAt(0).toUpperCase() : 'U'
  const canWrite = user?.role && user.role !== 'viewer'
  const quorum = inv.cluster?.quorum !== false
  const currentRoute = useMemo(() => parseResourceRoute(location.pathname), [location.pathname])
  const version = inv.host?.version

  return (
    <div className="pve-shell">
      <header className={`pve-header${collapsed ? ' sidebar-collapsed' : ''}`}>
        <div
          className={`pve-header-start${resizing ? ' resizing' : ''}`}
          style={!collapsed ? { width: `${treeWidth}px` } : undefined}
        >
          <button
            type="button"
            className="pve-icon-btn ghost"
            onClick={toggleSidebar}
            aria-label="Toggle resource tree"
          >
            <Icon name="panel-left" size={18} />
          </button>
          <Link to={resourceLink('dc', null, currentRoute)} className="pve-brand">
            <span className="brand-mark" aria-hidden>
              <Icon name="worker" size={16} />
            </span>
            <span className="pve-brand-copy">
              <span className="pve-brand-text">
                Pertisk <span className="accent">VM</span>
              </span>
              <span className="pve-brand-ver">{version ? `v${version}` : 'Virtual Environment'}</span>
            </span>
          </Link>
        </div>
        <div className="pve-header-main">
          <ResourceSearch cluster={inv.cluster} host={inv.host} vms={inv.vms} />
          <div className="pve-header-spacer" />
          <span className={`pve-quorum ${quorum ? 'ok' : 'bad'}`}>
            <Icon name={quorum ? 'check' : 'alert'} size={13} />
            {quorum ? 'Quorate' : 'No quorum'}
          </span>
          <div className="pve-header-actions">
            {canWrite && (
              <button type="button" className="pve-header-btn" onClick={() => setWizard(true)}>
                <Icon name="plus" size={15} />
                <span>Create guest</span>
              </button>
            )}
            <button
              type="button"
              className="pve-icon-btn ghost"
              onClick={inv.refresh}
              onMouseDown={(e) => e.preventDefault()}
              title="Refresh"
              aria-label="Refresh"
            >
              <Icon name="refresh" size={16} />
            </button>
            <button type="button" className="pve-icon-btn ghost" title="Help" aria-label="Help">
              <Icon name="help" size={16} />
            </button>
            <button
              type="button"
              className="pve-icon-btn"
              onClick={toggleAppearance}
              title={appearance === 'dark' ? 'Switch to light' : 'Switch to dark'}
              aria-label="Toggle color theme"
            >
              <Icon name={appearance === 'dark' ? 'sun' : 'moon'} size={16} />
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
                  <button type="button" onClick={logout}>
                    <Icon name="logout" size={16} />
                    Sign out
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      </header>

      <div className="pve-body">
        <div
          className={`sidebar-backdrop${mobileOpen ? ' open' : ''}`}
          aria-hidden={!mobileOpen}
          onClick={() => setMobileOpen(false)}
        />

        <aside
          ref={treeRef}
          className={`pve-tree${mobileOpen ? ' open' : ''}${collapsed ? ' collapsed' : ''}${
            resizing ? ' resizing' : ''
          }`}
          style={collapsed ? undefined : { width: `${treeWidth}px` }}
        >
          <ResourceTree
            cluster={inv.cluster}
            host={inv.host}
            vms={inv.vms}
          />
          {!collapsed && (
            <div
              className="pve-tree-resize"
              onPointerDown={onResizePointerDown}
              onDoubleClick={() => setTreeWidth(TREE_WIDTH_DEFAULT)}
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize sidebar"
              title="Drag to resize"
            />
          )}
        </aside>

        <div className={`pve-content${mobileOpen ? ' sidebar-open' : ''}`}>
          <main className="pve-main">
            <Outlet context={{ user, canWrite, inv }} />
          </main>
        </div>
      </div>

      {passwordOpen && <ChangePassword onClose={() => setPasswordOpen(false)} />}

      {reauth && (
        <Modal
          title="Session expired"
          hint="Sign in again to keep working. You will stay on this page."
          closable={false}
          footer={
            <>
              <button type="button" className="secondary" onClick={signOutExpired} disabled={reauthBusy}>
                Sign out
              </button>
              <button type="submit" form="reauth" disabled={reauthBusy || !reauthPass}>
                {reauthBusy ? 'Signing in…' : 'Sign in'}
              </button>
            </>
          }
        >
          {reauthError && <div className="error">{reauthError}</div>}
          <form id="reauth" onSubmit={reauthSubmit}>
            <div className="field">
              <label htmlFor="reauth-user">Username</label>
              <input
                id="reauth-user"
                value={reauthUser}
                onChange={(e) => setReauthUser(e.target.value)}
                autoComplete="username"
              />
            </div>
            <div className="field">
              <label htmlFor="reauth-pass">Password</label>
              <input
                id="reauth-pass"
                type="password"
                value={reauthPass}
                onChange={(e) => setReauthPass(e.target.value)}
                autoComplete="current-password"
                autoFocus
              />
            </div>
          </form>
        </Modal>
      )}

      {wizard && (
        <GuestWizard
          vms={inv.vms}
          volumes={inv.volumes}
          isos={inv.isos}
          networks={inv.networks}
          host={inv.host}
          cluster={inv.cluster}
          onClose={() => setWizard(false)}
          onCreated={async (vmId) => {
            await inv.refresh()
            setWizard(false)
            if (vmId != null && vmId !== '') nav(`/vm/${vmId}/console`)
          }}
        />
      )}
    </div>
  )
}
