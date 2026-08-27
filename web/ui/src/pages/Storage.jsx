import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, formatBytes, parseSize, replicasOf, snapshotsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'
import { useInventory } from '../useInventory'

export default function Storage() {
  const { canWrite } = useOutletContext()
  const { volumes, isos, cluster, error, setError, mutate } = useInventory()
  const confirm = useConfirm()
  const members = cluster?.members || []

  function nodeName(id) {
    return members.find((m) => m.id === id)?.name || (id ? String(id).slice(0, 8) : '—')
  }
  const [volOpen, setVolOpen] = useState(false)
  const [isoOpen, setIsoOpen] = useState(false)
  const [vol, setVol] = useState({ name: '', size: '8G', replicas: 2 })
  const [isoPath, setIsoPath] = useState('')
  const [busy, setBusy] = useState(false)
  const [action, setAction] = useState(null)
  const [actionName, setActionName] = useState('')
  const [actionSize, setActionSize] = useState('')
  const [actionLinked, setActionLinked] = useState(false)

  async function createVol(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await mutate(() =>
        api('/v1/volumes', {
          method: 'POST',
          body: {
            name: vol.name.trim(),
            size_bytes: parseSize(vol.size),
            format: 'raw',
            replicas: Number(vol.replicas) || undefined,
          },
        }),
      )
      setVol({ name: '', size: '8G', replicas: 2 })
      setVolOpen(false)
    } catch {
      /* inventory error */
    } finally {
      setBusy(false)
    }
  }

  async function importIso(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await mutate(() => api('/v1/isos', { method: 'POST', body: { path: isoPath.trim() } }))
      setIsoPath('')
      setIsoOpen(false)
    } catch {
      /* inventory error */
    } finally {
      setBusy(false)
    }
  }

  function openAction(type, v) {
    setAction({ type, vol: v })
    setActionName(type === 'clone' ? `${v.name}-copy` : type === 'snap' ? 'snap-1' : '')
    setActionSize(type === 'resize' ? String(Math.ceil((v.size_bytes || 0) / 1024 / 1024) + 'M') : '')
    setActionLinked(false)
  }

  async function runAction(e) {
    e.preventDefault()
    if (!action) return
    setBusy(true)
    const id = action.vol.id
    try {
      await mutate(async () => {
        if (action.type === 'resize') {
          await api(`/v1/volumes/${id}/resize`, {
            method: 'POST',
            body: { size_bytes: parseSize(actionSize) },
          })
        } else if (action.type === 'clone') {
          await api(`/v1/volumes/${id}/clone`, {
            method: 'POST',
            body: { name: actionName.trim(), linked: actionLinked },
          })
        } else if (action.type === 'snap') {
          await api(`/v1/volumes/${id}/snapshots`, {
            method: 'POST',
            body: { name: actionName.trim() },
          })
        } else if (action.type === 'restore') {
          await api(`/v1/volumes/${id}/snapshots/${encodeURIComponent(actionName)}/restore`, {
            method: 'POST',
          })
        }
      })
      setAction(null)
    } catch {
      /* inventory */
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="disk" size={20} />
            Storage
          </h1>
          <p className="dash-lead muted">Volumes replicate across nodes. ISOs are imported from a host path.</p>
        </div>
        {canWrite && (
          <div className="dash-resources-actions">
            <Btn icon="plus" variant="secondary" onClick={() => setIsoOpen(true)}>
              Import ISO
            </Btn>
            <Btn icon="plus" onClick={() => setVolOpen(true)}>
              Create volume
            </Btn>
          </div>
        )}
      </div>
      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}

      <section className="card table-card">
        <div className="table-meta">Volumes</div>
        {volumes.length === 0 ? (
          <p className="muted">No volumes yet.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Format</th>
                  <th>Size</th>
                  <th>Replicas</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {volumes.map((v) => {
                  const snaps = snapshotsOf(v)
                  const reps = replicasOf(v)
                  return (
                  <tr key={v.id}>
                    <td>
                      {v.name}
                      {snaps.length > 0 && (
                        <div className="muted">
                          {snaps.map((s) => s.name).join(', ')}
                        </div>
                      )}
                    </td>
                    <td className="mono-inline">{v.format}</td>
                    <td>{formatBytes(v.size_bytes)}</td>
                    <td>
                      {reps.length === 0
                        ? v.replica_count || 1
                        : reps.map((id) => nodeName(id)).join(', ')}
                    </td>
                    {canWrite && (
                      <td className="col-actions">
                        <div className="row-actions" style={{ marginTop: 0 }}>
                          <Btn variant="secondary" onClick={() => openAction('resize', v)}>
                            Resize
                          </Btn>
                          <Btn variant="secondary" onClick={() => openAction('snap', v)}>
                            Snapshot
                          </Btn>
                          {snaps.length > 0 && (
                            <Btn variant="secondary" onClick={() => openAction('restore', v)}>
                              Restore
                            </Btn>
                          )}
                          <Btn variant="secondary" onClick={() => openAction('clone', v)}>
                            Clone
                          </Btn>
                          <Btn
                            icon="trash"
                            variant="danger"
                            onClick={async () => {
                              const ok = await confirm({
                                title: 'Delete volume',
                                message: `Delete ${v.name}?`,
                                confirmLabel: 'Delete',
                              })
                              if (ok) mutate(() => api(`/v1/volumes/${v.id}`, { method: 'DELETE' }))
                            }}
                          >
                            Delete
                          </Btn>
                        </div>
                      </td>
                    )}
                  </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="card table-card">
        <div className="table-meta">ISOs</div>
        {isos.length === 0 ? (
          <p className="muted">No ISOs imported.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Size</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {isos.map((iso) => (
                  <tr key={iso.name}>
                    <td>{iso.name}</td>
                    <td>{formatBytes(iso.size_bytes)}</td>
                    {canWrite && (
                      <td className="col-actions">
                        <Btn
                          icon="trash"
                          variant="danger"
                          onClick={async () => {
                            const ok = await confirm({
                              title: 'Remove ISO',
                              message: `Remove ${iso.name}?`,
                              confirmLabel: 'Remove',
                            })
                            if (ok) mutate(() => api(`/v1/isos/${iso.name}`, { method: 'DELETE' }))
                          }}
                        >
                          Remove
                        </Btn>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {action && (
        <Modal
          title={
            action.type === 'resize'
              ? `Resize ${action.vol.name}`
              : action.type === 'clone'
                ? `Clone ${action.vol.name}`
                : action.type === 'restore'
                  ? `Restore ${action.vol.name}`
                  : `Snapshot ${action.vol.name}`
          }
          onClose={() => setAction(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setAction(null)}>
                Cancel
              </button>
              <button type="submit" form="vol-action" disabled={busy}>
                {busy ? 'Working…' : 'Apply'}
              </button>
            </>
          }
        >
          <form id="vol-action" onSubmit={runAction}>
            {action.type === 'resize' && (
              <div className="field">
                <label htmlFor="act-size">New size</label>
                <input
                  id="act-size"
                  required
                  value={actionSize}
                  onChange={(e) => setActionSize(e.target.value)}
                  placeholder="16G"
                />
              </div>
            )}
            {action.type === 'clone' && (
              <>
                <div className="field">
                  <label htmlFor="act-clone">New name</label>
                  <input
                    id="act-clone"
                    required
                    value={actionName}
                    onChange={(e) => setActionName(e.target.value)}
                  />
                </div>
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={actionLinked}
                    onChange={(e) => setActionLinked(e.target.checked)}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">Linked clone (qcow2 backing file)</span>
                </label>
              </>
            )}
            {action.type === 'snap' && (
              <div className="field">
                <label htmlFor="act-snap">Snapshot name</label>
                <input
                  id="act-snap"
                  required
                  value={actionName}
                  onChange={(e) => setActionName(e.target.value)}
                />
              </div>
            )}
            {action.type === 'restore' && (
              <div className="field">
                <label htmlFor="act-restore">Snapshot</label>
                <select
                  id="act-restore"
                  value={actionName}
                  onChange={(e) => setActionName(e.target.value)}
                  required
                >
                  {snapshotsOf(action.vol).map((s) => (
                    <option key={s.name} value={s.name}>
                      {s.name}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </form>
        </Modal>
      )}

      {volOpen && (
        <Modal
          title="Create volume"
          onClose={() => setVolOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setVolOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="create-vol" disabled={busy}>
                {busy ? 'Creating…' : 'Create'}
              </button>
            </>
          }
        >
          <form id="create-vol" onSubmit={createVol}>
            <div className="field">
              <label htmlFor="vol-name">Name</label>
              <input
                id="vol-name"
                required
                value={vol.name}
                onChange={(e) => setVol({ ...vol, name: e.target.value })}
              />
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="vol-size">Size</label>
                <input
                  id="vol-size"
                  value={vol.size}
                  onChange={(e) => setVol({ ...vol, size: e.target.value })}
                  placeholder="8G"
                />
              </div>
              <div className="field">
                <label htmlFor="vol-rep">Replicas</label>
                <input
                  id="vol-rep"
                  type="number"
                  min="1"
                  value={vol.replicas}
                  onChange={(e) => setVol({ ...vol, replicas: e.target.value })}
                />
              </div>
            </div>
          </form>
        </Modal>
      )}

      {isoOpen && (
        <Modal
          title="Import ISO"
          hint="Path must exist on this node."
          onClose={() => setIsoOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setIsoOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="import-iso" disabled={busy}>
                Import
              </button>
            </>
          }
        >
          <form id="import-iso" onSubmit={importIso}>
            <div className="field">
              <label htmlFor="iso-path">Host path</label>
              <input
                id="iso-path"
                required
                value={isoPath}
                onChange={(e) => setIsoPath(e.target.value)}
                placeholder="/path/to/os.iso"
              />
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
