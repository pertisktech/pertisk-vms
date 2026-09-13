import { useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { formatUnix, formatUptime } from '../api'
import { Icon } from './Icons'

const SEEN_KEY = 'pertisk_notify_seen_unix'

function eventTime(item) {
  return Number(item.finished_unix || item.created_unix || 0)
}

function mergeEvents(tasks, audit) {
  const out = []
  for (const t of tasks || []) {
    const status = String(t.status || '')
    const failed = status === 'error' || status === 'failed'
    out.push({
      id: `task-${t.id || t.kind}-${t.created_unix}`,
      kind: failed ? 'error' : status === 'running' ? 'pending' : 'ok',
      title: t.kind || 'task',
      detail: [t.target, t.error || t.actor].filter(Boolean).join(' · '),
      unix: eventTime(t),
      href: '/dc/tasks',
    })
  }
  for (const a of audit || []) {
    out.push({
      id: `audit-${a.id || a.action}-${a.created_unix}`,
      kind: 'ok',
      title: a.action || 'event',
      detail: [a.actor, a.target].filter(Boolean).join(' · '),
      unix: Number(a.created_unix || 0),
      href: '/dc/tasks',
    })
  }
  out.sort((a, b) => b.unix - a.unix)
  return out.slice(0, 12)
}

export default function StatusPanel({ host, tasks, audit }) {
  const wrapRef = useRef(null)
  const [open, setOpen] = useState(false)
  const [now, setNow] = useState(() => Date.now())
  const [seenUnix, setSeenUnix] = useState(() => {
    const stored = localStorage.getItem(SEEN_KEY)
    if (stored != null && stored !== '') return Number(stored) || 0
    const nowUnix = Math.floor(Date.now() / 1000)
    localStorage.setItem(SEEN_KEY, String(nowUnix))
    return nowUnix
  })
  const fetchedAt = useRef(Date.now())
  const baseUptime = useRef(Number(host?.uptime_secs) || 0)
  const baseDaemon = useRef(Number(host?.daemon_uptime_secs) || 0)

  useEffect(() => {
    fetchedAt.current = Date.now()
    baseUptime.current = Number(host?.uptime_secs) || 0
    baseDaemon.current = Number(host?.daemon_uptime_secs) || 0
  }, [host?.uptime_secs, host?.daemon_uptime_secs])

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [])

  useEffect(() => {
    function onPointerDown(e) {
      if (wrapRef.current && !wrapRef.current.contains(e.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    return () => document.removeEventListener('pointerdown', onPointerDown)
  }, [])

  const drift = Math.floor((now - fetchedAt.current) / 1000)
  const nodeUptime = baseUptime.current > 0 ? baseUptime.current + drift : 0
  const daemonUptime = baseDaemon.current > 0 ? baseDaemon.current + drift : 0
  const events = useMemo(() => mergeEvents(tasks, audit), [tasks, audit])
  const unread = events.filter((e) => e.unix > seenUnix).length
  const errors = events.filter((e) => e.kind === 'error').length

  function toggle() {
    setOpen((v) => {
      const next = !v
      if (next) {
        const latest = events[0]?.unix || Math.floor(Date.now() / 1000)
        setSeenUnix(latest)
        localStorage.setItem(SEEN_KEY, String(latest))
        setNow(Date.now())
      }
      return next
    })
  }

  return (
    <div className="status-panel" ref={wrapRef}>
      <span className="pve-uptime" title={`Node up ${nodeUptime ? formatUptime(nodeUptime, { long: true }) : 'unknown'}`}>
        <Icon name="clock" size={13} />
        {nodeUptime ? formatUptime(nodeUptime) : '—'}
      </span>
      <button
        type="button"
        className={`pve-icon-btn ghost${open ? ' open' : ''}`}
        title="Uptime and notifications"
        aria-label="Uptime and notifications"
        aria-expanded={open}
        onClick={toggle}
      >
        <Icon name="help" size={16} />
        {unread > 0 && (
          <span className={`status-panel-badge${errors ? ' hot' : ''}`} aria-hidden>
            {unread > 9 ? '9+' : unread}
          </span>
        )}
      </button>
      {open && (
        <div className="status-panel-dropdown">
          <div className="status-panel-section">
            <div className="status-panel-label">Uptime</div>
            <div className="status-panel-uptime">
              <div>
                <span className="muted">Node</span>
                <strong>{nodeUptime ? formatUptime(nodeUptime, { long: true }) : '—'}</strong>
              </div>
              <div>
                <span className="muted">Control plane</span>
                <strong>{daemonUptime ? formatUptime(daemonUptime, { long: true }) : '—'}</strong>
              </div>
            </div>
          </div>
          <div className="status-panel-section">
            <div className="status-panel-label">Notifications</div>
            {events.length === 0 ? (
              <p className="status-panel-empty muted">No recent events.</p>
            ) : (
              <ul className="status-panel-list">
                {events.map((ev) => (
                  <li key={ev.id}>
                    <Link to={ev.href} className={`status-panel-item ${ev.kind}`} onClick={() => setOpen(false)}>
                      <span className={`status-panel-dot ${ev.kind}`} />
                      <span className="status-panel-copy">
                        <strong>{ev.title}</strong>
                        {ev.detail && <span className="muted">{ev.detail}</span>}
                      </span>
                      <span className="status-panel-when muted">{formatUnix(ev.unix)}</span>
                    </Link>
                  </li>
                ))}
              </ul>
            )}
            <Link to="/dc/tasks" className="status-panel-more" onClick={() => setOpen(false)}>
              View task history
            </Link>
          </div>
        </div>
      )}
    </div>
  )
}
