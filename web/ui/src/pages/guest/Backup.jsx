import { useCallback, useEffect, useState } from 'react'
import { api, formatBytes, formatUnix } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

export default function GuestBackup() {
  const { vm, vmId, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const [backups, setBackups] = useState([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const running = vm?.state === 'running'

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const list = await api(`/v1/vms/${vmId}/backups`)
      setBackups(Array.isArray(list) ? list : [])
      setError('')
    } catch (err) {
      setError(err.message || String(err))
      setBackups([])
    } finally {
      setLoading(false)
    }
  }, [vmId])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function backupNow() {
    if (running) {
      setError('Stop the guest before creating a backup.')
      return
    }
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backups`, { method: 'POST', body: {} })
      await inv.refresh()
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function removeBackup(id) {
    const ok = await confirm({
      title: 'Delete backup',
      message: 'Remove this backup from disk? This cannot be undone.',
      confirmLabel: 'Delete',
    })
    if (!ok) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backups/${encodeURIComponent(id)}`, { method: 'DELETE' })
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="pve-tab-page">
      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Backup schedule</h3>
        </header>
        <dl className="pve-kv pve-backup-kv">
          <dt>Job</dt>
          <dd className="mono-inline muted">Not configured</dd>
          <dt>Schedule</dt>
          <dd className="mono-inline muted">—</dd>
          <dt>Storage</dt>
          <dd className="mono-inline">local backups/</dd>
          <dt>Mode</dt>
          <dd className="mono-inline muted">manual export (guest stopped)</dd>
          <dt>Retention</dt>
          <dd className="mono-inline muted">Keep all</dd>
        </dl>
      </section>

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Backups</h3>
          <div className="pve-card-panel-actions">
            {canWrite && (
              <Btn
                icon="archive"
                onClick={backupNow}
                disabled={busy || running}
                title={running ? 'Stop the guest first' : 'Export disks now'}
              >
                {busy ? 'Working…' : 'Backup now'}
              </Btn>
            )}
          </div>
        </header>
        {loading ? (
          <p className="muted" style={{ padding: '0.85rem 1rem' }}>
            Loading…
          </p>
        ) : backups.length === 0 ? (
          <div className="pve-empty">
            <Icon name="archive" size={22} />
            <span className="muted">No backups yet.</span>
          </div>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Date</th>
                  <th>Name</th>
                  <th>Size</th>
                  <th>Disks</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {backups.map((b) => (
                  <tr key={b.id}>
                    <td className="mono-inline">{formatUnix(b.created_unix)}</td>
                    <td className="mono-inline">{b.name || b.id}</td>
                    <td className="mono-inline">{formatBytes(b.size_bytes)}</td>
                    <td className="muted">{(b.disks || []).length}</td>
                    <td className="pve-hw-act">
                      {canWrite && (
                        <Btn
                          icon="trash"
                          variant="danger"
                          disabled={busy}
                          onClick={() => removeBackup(b.id)}
                        >
                          Delete
                        </Btn>
                      )}
                    </td>
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
