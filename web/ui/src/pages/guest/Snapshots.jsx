import { useMemo, useState } from 'react'
import { api, formatUnix, snapshotsOf } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

function osDisks(vm, volumes) {
  const byId = new Map((volumes || []).map((v) => [String(v.id), v]))
  return (vm?.spec?.disks || [])
    .filter((d) => !d.cdrom && d.volume_id)
    .map((d) => byId.get(String(d.volume_id)))
    .filter(Boolean)
}

export default function GuestSnapshots() {
  const { vm, vmId, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const disks = useMemo(() => osDisks(vm, inv.volumes), [vm, inv.volumes])
  const [dialog, setDialog] = useState(false)
  const [schedDialog, setSchedDialog] = useState(false)
  const [name, setName] = useState('')
  const [desc, setDesc] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState(null)
  const [form, setForm] = useState({ enabled: true, interval_hours: 24, keep: 3 })
  const running = vm?.state === 'running'
  const schedule = vm?.spec?.snapshot || null

  const entries = useMemo(() => {
    const byName = new Map()
    for (const vol of disks) {
      for (const snap of snapshotsOf(vol)) {
        const key = snap.name
        const cur = byName.get(key) || {
          name: key,
          created_unix: snap.created_unix || 0,
          volumes: [],
        }
        cur.created_unix = Math.max(cur.created_unix, snap.created_unix || 0)
        cur.volumes.push({ id: vol.id, name: vol.name })
        byName.set(key, cur)
      }
    }
    return [...byName.values()].sort((a, b) => (b.created_unix || 0) - (a.created_unix || 0))
  }, [disks])

  async function takeSnapshot(e) {
    e.preventDefault()
    const snapName = name.trim()
    if (!snapName) {
      setError('Name is required.')
      return
    }
    if (disks.length === 0) {
      setError('This guest has no disks to snapshot.')
      return
    }
    setBusy(true)
    setError('')
    try {
      await inv.mutate(async () => {
        for (const vol of disks) {
          await api(`/v1/volumes/${vol.id}/snapshots`, {
            method: 'POST',
            body: { name: snapName },
          })
        }
      })
      setDialog(false)
      setName('')
      setDesc('')
      setSelected(snapName)
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  function openSchedule() {
    setError('')
    setForm({
      enabled: schedule ? Boolean(schedule.enabled) : true,
      interval_hours: Number(schedule?.interval_hours) || 24,
      keep: schedule?.keep == null ? 3 : Number(schedule.keep),
    })
    setSchedDialog(true)
  }

  async function saveSchedule(e) {
    e?.preventDefault?.()
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/snapshot-schedule`, {
        method: 'PUT',
        body: {
          enabled: Boolean(form.enabled),
          interval_hours: Math.max(1, Number(form.interval_hours) || 24),
          keep: Math.max(0, Number(form.keep) || 0),
        },
      })
      setSchedDialog(false)
      await inv.refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function clearSchedule() {
    const ok = await confirm({
      title: 'Clear snapshot schedule',
      message: 'Remove the automatic snapshot job for this guest? Existing snapshots stay.',
      confirmLabel: 'Clear',
    })
    if (!ok) return
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${vmId}/snapshot-schedule`, { method: 'DELETE' })
      setSchedDialog(false)
      await inv.refresh()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function rollback() {
    if (!selected) return
    if (running) {
      setError('Stop the guest before rolling back a snapshot.')
      return
    }
    const ok = await confirm({
      title: 'Rollback snapshot',
      message: `Restore disks to “${selected}”? Current disk state will be replaced.`,
      confirmLabel: 'Rollback',
    })
    if (!ok) return
    setBusy(true)
    setError('')
    try {
      await inv.mutate(async () => {
        for (const vol of disks) {
          const has = snapshotsOf(vol).some((s) => s.name === selected)
          if (!has) continue
          await api(`/v1/volumes/${vol.id}/snapshots/${encodeURIComponent(selected)}/restore`, {
            method: 'POST',
          })
        }
      })
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
          <h3>Snapshot schedule</h3>
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
          <dd className={`mono-inline ${schedule?.enabled ? '' : 'muted'}`}>
            {!schedule ? 'Not configured' : schedule.enabled ? 'Enabled' : 'Disabled'}
          </dd>
          <dt>Schedule</dt>
          <dd className="mono-inline muted">{schedule ? `every ${schedule.interval_hours || 24}h` : '—'}</dd>
          <dt>Last run</dt>
          <dd className="mono-inline muted">
            {schedule?.last_run_unix ? formatUnix(schedule.last_run_unix) : schedule ? 'never' : '—'}
          </dd>
          <dt>Retention</dt>
          <dd className="mono-inline">
            {!schedule ? '—' : schedule.keep ? `Keep ${schedule.keep}` : 'Keep all'}
          </dd>
          <dt>Mode</dt>
          <dd className="mono-inline muted">automatic, including while the guest is running</dd>
        </dl>
        <p className="muted">
          Named snapshots you take yourself are kept. Retention only removes snapshots named sched-.
        </p>
      </section>

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>Snapshots</h3>
          <div className="pve-card-panel-actions">
            {canWrite && (
              <>
                <Btn
                  icon="refresh"
                  variant="secondary"
                  disabled={!selected || busy || running}
                  onClick={rollback}
                  title={running ? 'Stop the guest to rollback' : 'Rollback selected snapshot'}
                >
                  Rollback
                </Btn>
                <Btn icon="camera" onClick={() => { setError(''); setDialog(true) }} disabled={busy || disks.length === 0}>
                  Take Snapshot
                </Btn>
              </>
            )}
          </div>
        </header>

        {disks.length === 0 ? (
          <div className="pve-empty">
            <Icon name="camera" size={22} />
            <span className="muted">No disks attached to snapshot.</span>
          </div>
        ) : (
          <div className="pve-snap-timeline">
            <button
              type="button"
              className={`pve-snap-now ${selected == null ? 'active' : ''}`}
              onClick={() => setSelected(null)}
            >
              <span className="pve-snap-dot">
                <Icon name="circle-dot" size={14} />
              </span>
              <span className="pve-snap-body">
                <strong>NOW</strong>
                <span className="muted">Current state</span>
              </span>
            </button>
            {entries.length === 0 ? (
              <p className="muted pve-snap-empty">No snapshots yet.</p>
            ) : (
              entries.map((snap) => (
                <button
                  type="button"
                  key={snap.name}
                  className={`pve-snap-item ${selected === snap.name ? 'active' : ''}`}
                  onClick={() => setSelected(snap.name)}
                >
                  <Icon name="camera" size={16} />
                  <span className="pve-snap-body">
                    <span className="pve-snap-title">
                      <strong>{snap.name}</strong>
                      <span className="mono-inline muted">{formatUnix(snap.created_unix)}</span>
                    </span>
                    <span className="muted">
                      {snap.volumes.map((v) => v.name).join(', ')}
                    </span>
                  </span>
                </button>
              ))
            )}
          </div>
        )}
      </section>

      {schedDialog && (
        <Modal
          title="Edit snapshot schedule"
          hint="Runs on this guest's disks. A running qcow2, RBD, or ZFS disk is snapshotted in place. A raw disk waits until the guest is stopped."
          onClose={() => setSchedDialog(false)}
          footer={
            <>
              {schedule && (
                <button type="button" className="secondary" onClick={clearSchedule} disabled={busy}>
                  Clear
                </button>
              )}
              <button type="button" className="secondary" onClick={() => setSchedDialog(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="snap-sched-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          {error && <div className="error">{error}</div>}
          <form id="snap-sched-form" onSubmit={saveSchedule}>
            <label className="chk">
              <input
                type="checkbox"
                checked={form.enabled}
                onChange={(e) => setForm((f) => ({ ...f, enabled: e.target.checked }))}
              />
              <span className="chk-box" />
              <span className="chk-label">Enabled</span>
            </label>
            <div className="field">
              <label htmlFor="snap-every">Every (hours)</label>
              <input
                id="snap-every"
                type="number"
                min="1"
                value={form.interval_hours}
                onChange={(e) => setForm((f) => ({ ...f, interval_hours: e.target.value }))}
              />
            </div>
            <div className="field">
              <label htmlFor="snap-keep">Keep</label>
              <input
                id="snap-keep"
                type="number"
                min="0"
                value={form.keep}
                onChange={(e) => setForm((f) => ({ ...f, keep: e.target.value }))}
              />
              <p className="field-hint">Newest scheduled snapshots to keep. Zero keeps all.</p>
            </div>
          </form>
        </Modal>
      )}

      {dialog && (
        <Modal
          title="Take snapshot"
          hint={
            running
              ? 'Snapshots each OS disk while the guest keeps running. Rollback still needs the guest stopped.'
              : 'Creates a snapshot of each OS disk. Rollback replaces the current disk contents.'
          }
          onClose={() => setDialog(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setDialog(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="snap-form" disabled={busy}>
                {busy ? 'Saving…' : 'Create'}
              </button>
            </>
          }
        >
          {error && <div className="error">{error}</div>}
          <form id="snap-form" onSubmit={takeSnapshot}>
            <div className="field">
              <label htmlFor="snap-name">Name</label>
              <input
                id="snap-name"
                required
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="pre-upgrade"
              />
            </div>
            <div className="field">
              <label htmlFor="snap-desc">Description (optional)</label>
              <input
                id="snap-desc"
                value={desc}
                onChange={(e) => setDesc(e.target.value)}
                placeholder="Before kernel upgrade"
              />
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
