import { useState } from 'react'
import { formatUnix } from '../api'
import { Icon } from './Icons'

const HEIGHT_KEY = 'pertisk_vm_tasklog_open'

function statusClass(status) {
  if (status === 'ok' || status === 'succeeded' || status === 'done') return 'ready'
  if (status === 'error' || status === 'failed') return 'error'
  return 'pending'
}

export default function TaskLog({ tasks }) {
  const [open, setOpen] = useState(() => localStorage.getItem(HEIGHT_KEY) !== '0')

  function toggle() {
    setOpen((v) => {
      localStorage.setItem(HEIGHT_KEY, v ? '0' : '1')
      return !v
    })
  }

  const failed = tasks.filter((t) => statusClass(t.status) === 'error').length

  return (
    <section className={`tasklog${open ? ' open' : ''}`}>
      <button type="button" className="tasklog-head" onClick={toggle}>
        <Icon name="clock" size={14} />
        <span>Tasks</span>
        <span className="tasklog-count">{tasks.length}</span>
        <span className="tasklog-spacer" />
        {failed > 0 && <span className="badge error">{failed} failed</span>}
        <Icon name={open ? 'chevron-down' : 'chevron-up'} size={14} />
      </button>
      {open && (
        <div className="tasklog-body">
          {tasks.length === 0 ? (
            <p className="muted tasklog-empty">No tasks yet.</p>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>Start time</th>
                  <th>Node</th>
                  <th>User</th>
                  <th>Description</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {tasks.slice(0, 40).map((t, i) => (
                  <tr key={t.id || `${t.kind}-${t.target}-${i}`}>
                    <td className="muted">{formatUnix(t.created_unix)}</td>
                    <td className="mono-inline">{t.node || '—'}</td>
                    <td>{t.actor || '—'}</td>
                    <td>
                      {t.kind}
                      {t.target ? <span className="muted"> · {t.target}</span> : null}
                    </td>
                    <td>
                      <span className={`badge ${statusClass(t.status)}`}>{t.status}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </section>
  )
}
