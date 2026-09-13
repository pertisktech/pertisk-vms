import { useMemo } from 'react'
import { formatUnix } from '../../api'
import { Icon } from '../../components/Icons'
import { useGuest } from '../GuestView'

function matchesGuest(item, vm) {
  if (!vm) return false
  const id = String(vm.id).toLowerCase()
  const name = String(vm.spec?.name || '').toLowerCase()
  const hay = [item?.target, item?.kind, item?.action, item?.error]
    .map((x) => String(x || '').toLowerCase())
    .join(' ')
  if (!hay.trim()) return false
  if (hay.includes(id)) return true
  if (name && hay.includes(name)) return true
  return false
}

function durationLabel(task) {
  const start = Number(task.created_unix) || 0
  const end = Number(task.finished_unix) || 0
  if (!start) return '—'
  if (!end) return task.status === 'running' ? 'running' : '—'
  const secs = Math.max(0, end - start)
  if (secs < 60) return `${secs}s`
  const m = Math.floor(secs / 60)
  const s = secs % 60
  return s ? `${m}m ${s}s` : `${m}m`
}

export default function GuestHistory() {
  const { vm, inv } = useGuest()
  const tasks = useMemo(
    () => (inv.tasks || []).filter((t) => matchesGuest(t, vm)),
    [inv.tasks, vm],
  )
  const audit = useMemo(
    () => (inv.audit || []).filter((a) => matchesGuest(a, vm)),
    [inv.audit, vm],
  )

  return (
    <div className="pve-tab-page">
      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Task history</h3>
        </header>
        {tasks.length === 0 ? (
          <div className="pve-empty">
            <Icon name="clock" size={22} />
            <span className="muted">No recent tasks for this guest.</span>
          </div>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Start Time</th>
                  <th>Duration</th>
                  <th>User</th>
                  <th>Description</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {tasks.map((t) => (
                  <tr key={t.id || `${t.kind}-${t.created_unix}`}>
                    <td className="mono-inline">{formatUnix(t.created_unix)}</td>
                    <td className="mono-inline">{durationLabel(t)}</td>
                    <td>{t.actor || '—'}</td>
                    <td>
                      {t.kind}
                      {t.target ? ` · ${t.target}` : ''}
                      {t.error ? ` — ${t.error}` : ''}
                    </td>
                    <td>
                      <span
                        className={`badge ${
                          t.status === 'ok' || t.status === 'succeeded' ? 'ready' : t.status
                        }`}
                      >
                        {t.status}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Audit</h3>
        </header>
        {audit.length === 0 ? (
          <p className="muted" style={{ padding: '0.85rem 1rem' }}>
            No audit events for this guest.
          </p>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Actor</th>
                  <th>Action</th>
                  <th>Target</th>
                </tr>
              </thead>
              <tbody>
                {audit.map((a, i) => (
                  <tr key={a.id || `${a.actor}-${a.action}-${i}`}>
                    <td className="mono-inline muted">{formatUnix(a.created_unix)}</td>
                    <td>{a.actor}</td>
                    <td>{a.action}</td>
                    <td className="mono-inline">{a.target || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
