import { useCallback, useEffect, useState } from 'react'
import { api, formatBytes, formatUnix } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

export default function GuestBackup() {
  const { vm, vmId, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const [backups, setBackups] = useState([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [dialog, setDialog] = useState(false)
  const [backupOpen, setBackupOpen] = useState(false)
  const [backupNotes, setBackupNotes] = useState('')
  const [notesEdit, setNotesEdit] = useState(null)
  const [editNotes, setEditNotes] = useState('')
  const [rename, setRename] = useState(null)
  const [renameName, setRenameName] = useState('')
  const [form, setForm] = useState({ enabled: true, interval_hours: 24, keep: 3 })
  const running = vm?.state === 'running'
  const schedule = vm?.spec?.backup || null

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

  function openSchedule() {
    setError('')
    setForm({
      enabled: schedule ? Boolean(schedule.enabled) : true,
      interval_hours: Number(schedule?.interval_hours) || 24,
      keep: schedule?.keep == null ? 3 : Number(schedule.keep),
    })
    setDialog(true)
  }

  async function saveSchedule(e) {
    e?.preventDefault?.()
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backup-schedule`, {
        method: 'PUT',
        body: {
          enabled: Boolean(form.enabled),
          interval_hours: Math.max(1, Number(form.interval_hours) || 24),
          keep: Math.max(0, Number(form.keep) || 0),
        },
      })
      setDialog(false)
      await inv.refresh()
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function clearSchedule() {
    const ok = await confirm({
      title: 'Clear backup schedule',
      message: 'Remove the automatic backup job for this guest?',
      confirmLabel: 'Clear',
    })
    if (!ok) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backup-schedule`, { method: 'DELETE' })
      setDialog(false)
      await inv.refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  function openBackupNow() {
    if (running) {
      setError('Stop the guest before creating a backup.')
      return
    }
    setError('')
    setBackupNotes('')
    setBackupOpen(true)
  }

  async function backupNow(e) {
    e?.preventDefault?.()
    if (running) {
      setError('Stop the guest before creating a backup.')
      return
    }
    setBusy(true)
    setError('')
    try {
      const notes = backupNotes.trim()
      await api(`/v1/vms/${vmId}/backups`, {
        method: 'POST',
        body: notes ? { notes } : {},
      })
      setBackupOpen(false)
      setBackupNotes('')
      await inv.refresh()
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  function openNotes(backup) {
    setError('')
    setNotesEdit(backup)
    setEditNotes(backup.notes || '')
  }

  async function saveNotes(e) {
    e.preventDefault()
    if (!notesEdit) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backups/${encodeURIComponent(notesEdit.id)}`, {
        method: 'PATCH',
        body: { notes: editNotes },
      })
      setNotesEdit(null)
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  function openRename(backup) {
    setError('')
    setRename(backup)
    setRenameName(backup.name || backup.id)
  }

  async function runRename(e) {
    e.preventDefault()
    if (!rename) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backups/${encodeURIComponent(rename.id)}`, {
        method: 'PATCH',
        body: { name: renameName.trim() },
      })
      setRename(null)
      await refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function restoreBackup(backup) {
    const ok = await confirm({
      title: 'Restore backup',
      message: `Replace this guest's disks with ${backup.name || backup.id}? The guest stays stopped.`,
      confirmLabel: 'Restore',
    })
    if (!ok) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/backups/${encodeURIComponent(backup.id)}/restore`, {
        method: 'POST',
        body: {},
      })
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

  const jobLabel = !schedule ? 'Not configured' : schedule.enabled ? 'Enabled' : 'Disabled'
  const scheduleLabel = !schedule
    ? '—'
    : `every ${schedule.interval_hours || 24}h`
  const retentionLabel = !schedule
    ? 'Keep all'
    : schedule.keep
      ? `Keep ${schedule.keep}`
      : 'Keep all'
  const lastRun = schedule?.last_run_unix
    ? formatUnix(schedule.last_run_unix)
    : schedule
      ? 'never'
      : '—'

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
          {canWrite && (
            <div className="pve-card-panel-actions">
              <Btn icon="pencil" variant="secondary" onClick={openSchedule} disabled={busy}>
                Edit
              </Btn>
            </div>
          )}
        </header>
        <dl className="pve-kv pve-backup-kv">
          <dt>Job</dt>
          <dd className={`mono-inline ${schedule?.enabled ? '' : 'muted'}`}>{jobLabel}</dd>
          <dt>Schedule</dt>
          <dd className="mono-inline muted">{scheduleLabel}</dd>
          <dt>Last run</dt>
          <dd className="mono-inline muted">{lastRun}</dd>
          <dt>Storage</dt>
          <dd className="mono-inline">local backups/</dd>
          <dt>Mode</dt>
          <dd className="mono-inline muted">automatic when the guest is stopped (local disks)</dd>
          <dt>Retention</dt>
          <dd className="mono-inline muted">{retentionLabel}</dd>
        </dl>
      </section>

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Backups</h3>
          <div className="pve-card-panel-actions">
            {canWrite && (
              <Btn
                icon="archive"
                onClick={openBackupNow}
                disabled={busy || running}
                title={running ? 'Stop the guest first' : 'Export disks now'}
              >
                Backup now
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
                  <th>Notes</th>
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
                    <td className="muted">{b.notes || '—'}</td>
                    <td className="mono-inline">{formatBytes(b.size_bytes)}</td>
                    <td className="muted">{(b.disks || []).length}</td>
                    <td className="pve-hw-act">
                      {canWrite && (
                        <>
                          <Btn
                            variant="secondary"
                            disabled={busy}
                            title="Rename backup"
                            onClick={() => openRename(b)}
                          >
                            Rename
                          </Btn>
                          <Btn
                            icon="pencil"
                            variant="secondary"
                            disabled={busy}
                            title="Edit backup notes"
                            onClick={() => openNotes(b)}
                          >
                            Notes
                          </Btn>
                          <Btn
                            icon="refresh"
                            variant="secondary"
                            disabled={busy || running}
                            title={running ? 'Stop the guest first' : 'Replace the current disks'}
                            onClick={() => restoreBackup(b)}
                          >
                            Restore
                          </Btn>
                          <Btn
                            icon="trash"
                            variant="danger"
                            disabled={busy}
                            onClick={() => removeBackup(b.id)}
                          >
                            Delete
                          </Btn>
                        </>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {backupOpen && (
        <Modal
          title="Backup now"
          onClose={() => setBackupOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setBackupOpen(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="backup-now-form" disabled={busy || running}>
                {busy ? 'Working…' : 'Backup'}
              </button>
            </>
          }
        >
          <form id="backup-now-form" onSubmit={backupNow}>
            <div className="field">
              <label htmlFor="backup-notes">Notes</label>
              <textarea
                id="backup-notes"
                rows={3}
                autoFocus
                maxLength={500}
                value={backupNotes}
                onChange={(e) => setBackupNotes(e.target.value)}
                placeholder="Optional — why this backup was taken"
              />
              <p className="field-hint">Stored with the backup. The guest must stay stopped.</p>
            </div>
          </form>
        </Modal>
      )}

      {rename && (
        <Modal
          title={`Rename ${rename.name || rename.id}`}
          onClose={() => setRename(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setRename(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="backup-rename-form" disabled={busy || !renameName.trim()}>
                {busy ? 'Working…' : 'Apply'}
              </button>
            </>
          }
        >
          <form id="backup-rename-form" onSubmit={runRename}>
            <div className="field">
              <label htmlFor="backup-rename-name">Name</label>
              <input
                id="backup-rename-name"
                required
                autoFocus
                maxLength={128}
                value={renameName}
                onChange={(e) => setRenameName(e.target.value)}
              />
              <p className="field-hint">
                Only the display name changes. The backup id and files under backups/ stay put.
              </p>
            </div>
          </form>
        </Modal>
      )}

      {notesEdit && (
        <Modal
          title={`Notes for ${notesEdit.name || notesEdit.id}`}
          onClose={() => setNotesEdit(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setNotesEdit(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="backup-notes-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          <form id="backup-notes-form" onSubmit={saveNotes}>
            <div className="field">
              <label htmlFor="backup-edit-notes">Notes</label>
              <textarea
                id="backup-edit-notes"
                rows={3}
                autoFocus
                maxLength={500}
                value={editNotes}
                onChange={(e) => setEditNotes(e.target.value)}
                placeholder="What this backup is for"
              />
              <p className="field-hint">Leave empty to clear the note.</p>
            </div>
          </form>
        </Modal>
      )}

      {dialog && (
        <Modal
          title="Edit backup schedule"
          onClose={() => setDialog(false)}
          footer={
            <>
              {schedule && (
                <button type="button" className="secondary" onClick={clearSchedule} disabled={busy}>
                  Clear
                </button>
              )}
              <button type="button" className="secondary" onClick={() => setDialog(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="backup-schedule-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          {error && <div className="error">{error}</div>}
          <form id="backup-schedule-form" onSubmit={saveSchedule}>
            <label className="chk">
              <input
                type="checkbox"
                checked={form.enabled}
                onChange={(e) => setForm({ ...form, enabled: e.target.checked })}
              />
              <span className="chk-box" />
              <span className="chk-label">
                Enable schedule
                <small>Runs only while this guest is stopped. Running guests are skipped until the next tick.</small>
              </span>
            </label>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="backup-every">Every (hours)</label>
                <input
                  id="backup-every"
                  type="number"
                  min="1"
                  value={form.interval_hours}
                  onChange={(e) => setForm({ ...form, interval_hours: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="backup-keep">Keep</label>
                <input
                  id="backup-keep"
                  type="number"
                  min="0"
                  value={form.keep}
                  onChange={(e) => setForm({ ...form, keep: e.target.value })}
                />
                <p className="field-hint">Newest backups to keep after a scheduled run. Zero keeps all.</p>
              </div>
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
