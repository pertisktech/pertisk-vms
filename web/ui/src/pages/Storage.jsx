import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, formatBytes, isCloudInitIso, parseSize, replicasOf, snapshotsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'

export default function Storage() {
  const { canWrite, inv } = useOutletContext()
  const { volumes, isos, cluster, error, setError, mutate } = inv
  const confirm = useConfirm()
  const members = cluster?.members || []
  const osIsos = isos.filter((iso) => !isCloudInitIso(iso.name))
  const seedIsos = isos.filter((iso) => isCloudInitIso(iso.name))

  function nodeName(id) {
    return members.find((m) => m.id === id)?.name || (id ? String(id).slice(0, 8) : '—')
  }
  const [volOpen, setVolOpen] = useState(false)
  const [isoOpen, setIsoOpen] = useState(false)
  const [vol, setVol] = useState({ name: '', size: '8G', backend: 'replica', notes: '' })
  const [isoPath, setIsoPath] = useState('')
  const [isoFile, setIsoFile] = useState(null)
  const [isoMode, setIsoMode] = useState('file')
  const [isoNotes, setIsoNotes] = useState('')
  const [ci, setCi] = useState({ name: 'cidata.iso', hostname: '', user: 'ubuntu', password: '', sshKey: '' })
  const [busy, setBusy] = useState(false)
  const [action, setAction] = useState(null)
  const [actionName, setActionName] = useState('')
  const [actionNotes, setActionNotes] = useState('')
  const [actionSize, setActionSize] = useState('')
  const [actionLinked, setActionLinked] = useState(false)
  const [notesEdit, setNotesEdit] = useState(null)
  const [editNotes, setEditNotes] = useState('')

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
            backend: vol.backend,
            notes: vol.notes.trim() || undefined,
          },
        }),
      )
      setVol({ name: '', size: '8G', backend: 'replica', notes: '' })
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
    const notes = isoNotes.trim() || undefined
    try {
      await mutate(async () => {
        if (isoMode === 'file') {
          if (!isoFile) throw new Error('Choose an ISO file')
          const qs = new URLSearchParams({ name: isoFile.name })
          if (notes) qs.set('notes', notes)
          await api(`/v1/isos/upload?${qs}`, {
            method: 'POST',
            headers: { 'content-type': 'application/octet-stream' },
            body: isoFile,
          })
        } else if (isoMode === 'cloudinit') {
          await api('/v1/isos/cloud-init', {
            method: 'POST',
            body: {
              name: ci.name.trim() || `${ci.hostname.trim() || 'guest'}-cidata.iso`,
              hostname: ci.hostname.trim() || undefined,
              user: ci.user.trim() || undefined,
              password: ci.password || undefined,
              ssh_authorized_keys: ci.sshKey
                .split('\n')
                .map((s) => s.trim())
                .filter(Boolean),
              ...(notes ? { notes } : {}),
            },
          })
        } else {
          await api('/v1/isos', {
            method: 'POST',
            body: { path: isoPath.trim(), ...(notes ? { notes } : {}) },
          })
        }
      })
      setIsoPath('')
      setIsoFile(null)
      setIsoNotes('')
      setCi({ name: 'cidata.iso', hostname: '', user: 'ubuntu', password: '', sshKey: '' })
      setIsoOpen(false)
    } catch {
      /* inventory error */
    } finally {
      setBusy(false)
    }
  }

  function openIsoNotes(iso) {
    setNotesEdit(iso)
    setEditNotes(iso.notes || '')
  }

  async function saveIsoNotes(e) {
    e.preventDefault()
    if (!notesEdit) return
    setBusy(true)
    try {
      await mutate(() =>
        api(`/v1/isos/${encodeURIComponent(notesEdit.name)}`, {
          method: 'PATCH',
          body: { notes: editNotes },
        }),
      )
      setNotesEdit(null)
    } catch {
      /* inventory */
    } finally {
      setBusy(false)
    }
  }

  function openAction(type, v) {
    setAction({ type, vol: v })
    const snaps = snapshotsOf(v)
    setActionName(
      type === 'clone'
        ? `${v.name}-copy`
        : type === 'rename'
          ? v.name
          : type === 'notes'
            ? v.notes || ''
            : type === 'snap'
              ? 'snap-1'
              : snaps[0]?.name || '',
    )
    setActionNotes(
      type === 'snapnotes' ? snaps[0]?.notes || '' : type === 'snap' ? '' : '',
    )
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
        } else if (action.type === 'rename') {
          await api(`/v1/volumes/${id}/rename`, {
            method: 'POST',
            body: { name: actionName.trim() },
          })
        } else if (action.type === 'notes') {
          await api(`/v1/volumes/${id}`, {
            method: 'PATCH',
            body: { notes: actionName },
          })
        } else if (action.type === 'clone') {
          await api(`/v1/volumes/${id}/clone`, {
            method: 'POST',
            body: { name: actionName.trim(), linked: actionLinked },
          })
        } else if (action.type === 'snap') {
          const notes = actionNotes.trim()
          await api(`/v1/volumes/${id}/snapshots`, {
            method: 'POST',
            body: { name: actionName.trim(), ...(notes ? { notes } : {}) },
          })
        } else if (action.type === 'snapnotes') {
          await api(`/v1/volumes/${id}/snapshots/${encodeURIComponent(actionName)}`, {
            method: 'PATCH',
            body: { notes: actionNotes },
          })
        } else if (action.type === 'restore') {
          await api(`/v1/volumes/${id}/snapshots/${encodeURIComponent(actionName)}/restore`, {
            method: 'POST',
          })
        } else if (action.type === 'drop') {
          await api(`/v1/volumes/${id}/snapshots/${encodeURIComponent(actionName)}`, {
            method: 'DELETE',
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
    <div className="pve-tab-page">
      {canWrite && (
        <div className="pve-action-row">
          <Btn icon="plus" variant="secondary" onClick={() => setIsoOpen(true)}>
            Import ISO
          </Btn>
          <Btn icon="plus" onClick={() => setVolOpen(true)}>
            Create volume
          </Btn>
        </div>
      )}
      <div className="banner">
        Local volumes stay on this node. Choose Shared (Ceph) when the disk must restart on another node.
      </div>
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
          <h3>Volumes</h3>
        </header>
        {volumes.length === 0 ? (
          <div className="pve-empty">
            <Icon name="disk" size={22} />
            <span className="muted">No volumes yet.</span>
          </div>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Format</th>
                  <th>Size</th>
                  <th>Storage</th>
                  <th>Location</th>
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
                        <strong>{v.name}</strong>
                        {v.notes && <div className="muted">{v.notes}</div>}
                        {snaps.length > 0 && (
                          <div className="muted">
                            {snaps
                              .map((s) => (s.notes ? `${s.name} (${s.notes})` : s.name))
                              .join(', ')}
                          </div>
                        )}
                      </td>
                      <td className="mono-inline muted">{v.format}</td>
                      <td className="mono-inline muted">{formatBytes(v.size_bytes)}</td>
                      <td className="mono-inline muted">
                        {v.backend === 'rbd' ? 'rbd' : v.backend === 'zfs' ? 'zfs' : 'local'}
                      </td>
                      <td className="mono-inline muted">
                        {v.backend === 'rbd'
                          ? 'shared'
                          : v.backend === 'zfs'
                            ? 'this node (zvol)'
                            : reps.length === 0
                              ? 'this node'
                              : reps.map((id) => nodeName(id)).join(', ')}
                      </td>
                      {canWrite && (
                        <td className="col-actions">
                          <div className="row-actions">
                            <Btn variant="secondary" onClick={() => openAction('rename', v)}>
                              Rename
                            </Btn>
                            <Btn variant="secondary" onClick={() => openAction('notes', v)}>
                              Notes
                            </Btn>
                            <Btn variant="secondary" onClick={() => openAction('resize', v)}>
                              Resize
                            </Btn>
                            <Btn variant="secondary" onClick={() => openAction('snap', v)}>
                              Snapshot
                            </Btn>
                            {snaps.length > 0 && (
                              <Btn variant="secondary" onClick={() => openAction('snapnotes', v)}>
                                Snap notes
                              </Btn>
                            )}
                            {snaps.length > 0 && (
                              <Btn variant="secondary" onClick={() => openAction('restore', v)}>
                                Restore
                              </Btn>
                            )}
                            {snaps.length > 0 && (
                              <Btn variant="secondary" onClick={() => openAction('drop', v)}>
                                Remove snap
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

      <section className="pve-card-panel">
        <header className="pve-card-panel-head">
          <h3>ISOs</h3>
        </header>
        {osIsos.length === 0 ? (
          <p className="muted" style={{ padding: '0.85rem 1rem' }}>
            No installer ISOs imported.
          </p>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Size</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {osIsos.map((iso) => (
                  <tr key={iso.name}>
                    <td>
                      {iso.name}
                      {iso.notes && <div className="muted">{iso.notes}</div>}
                    </td>
                    <td className="mono-inline muted">{formatBytes(iso.size_bytes)}</td>
                    {canWrite && (
                      <td className="col-actions">
                        <div className="row-actions">
                          <Btn variant="secondary" onClick={() => openIsoNotes(iso)}>
                            Notes
                          </Btn>
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
                        </div>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {seedIsos.length > 0 && (
        <section className="pve-card-panel">
          <header className="pve-card-panel-head">
            <h3>Cloud-init seeds</h3>
          </header>
          <p className="muted" style={{ margin: '0.75rem 1rem 0' }}>
            Tiny cidata ISOs created when you clone a cloud template. Not an OS image — import GenericCloud qcow2
            under Templates.
          </p>
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Size</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {seedIsos.map((iso) => (
                  <tr key={iso.name}>
                    <td>
                      {iso.name}
                      {iso.notes && <div className="muted">{iso.notes}</div>}
                    </td>
                    <td className="mono-inline muted">{formatBytes(iso.size_bytes)}</td>
                    {canWrite && (
                      <td className="col-actions">
                        <div className="row-actions">
                          <Btn variant="secondary" onClick={() => openIsoNotes(iso)}>
                            Notes
                          </Btn>
                          <Btn
                            icon="trash"
                            variant="danger"
                            onClick={async () => {
                              const ok = await confirm({
                                title: 'Remove seed',
                                message: `Remove ${iso.name}?`,
                                confirmLabel: 'Remove',
                              })
                              if (ok) mutate(() => api(`/v1/isos/${iso.name}`, { method: 'DELETE' }))
                            }}
                          >
                            Remove
                          </Btn>
                        </div>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      {action && (
        <Modal
          title={
            action.type === 'resize'
              ? `Resize ${action.vol.name}`
              : action.type === 'rename'
                ? `Rename ${action.vol.name}`
                : action.type === 'notes'
                  ? `Notes for ${action.vol.name}`
                  : action.type === 'clone'
                    ? `Clone ${action.vol.name}`
                    : action.type === 'snapnotes'
                      ? `Snapshot notes on ${action.vol.name}`
                      : action.type === 'restore'
                        ? `Restore ${action.vol.name}`
                        : action.type === 'drop'
                          ? `Remove snapshot from ${action.vol.name}`
                          : `Snapshot ${action.vol.name}`
          }
          onClose={() => setAction(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setAction(null)}>
                Cancel
              </button>
              <button type="submit" form="vol-action" disabled={busy}>
                {busy ? 'Working…' : action.type === 'drop' ? 'Remove' : 'Apply'}
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
            {action.type === 'rename' && (
              <div className="field">
                <label htmlFor="act-rename">Name</label>
                <input
                  id="act-rename"
                  required
                  autoFocus
                  value={actionName}
                  onChange={(e) => setActionName(e.target.value)}
                />
                <p className="field-hint">
                  Local disk files stay on the same path. An RBD or ZFS volume renames the image while no guest is running it.
                </p>
              </div>
            )}
            {action.type === 'notes' && (
              <div className="field">
                <label htmlFor="act-notes">Notes</label>
                <textarea
                  id="act-notes"
                  rows={4}
                  autoFocus
                  maxLength={500}
                  value={actionName}
                  onChange={(e) => setActionName(e.target.value)}
                  placeholder="What this volume is for"
                />
                <p className="field-hint">
                  Shown on Storage. Leave empty to clear the note.
                </p>
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
              <>
                <div className="field">
                  <label htmlFor="act-snap">Snapshot name</label>
                  <input
                    id="act-snap"
                    required
                    autoFocus
                    value={actionName}
                    onChange={(e) => setActionName(e.target.value)}
                  />
                </div>
                <div className="field">
                  <label htmlFor="act-snap-notes">Description (optional)</label>
                  <textarea
                    id="act-snap-notes"
                    rows={3}
                    maxLength={500}
                    value={actionNotes}
                    onChange={(e) => setActionNotes(e.target.value)}
                    placeholder="Before resize"
                  />
                </div>
              </>
            )}
            {action.type === 'snapnotes' && (
              <>
                <div className="field">
                  <label htmlFor="act-snapnotes-name">Snapshot</label>
                  <select
                    id="act-snapnotes-name"
                    value={actionName}
                    onChange={(e) => {
                      const name = e.target.value
                      setActionName(name)
                      const snap = snapshotsOf(action.vol).find((s) => s.name === name)
                      setActionNotes(snap?.notes || '')
                    }}
                    required
                  >
                    {snapshotsOf(action.vol).map((s) => (
                      <option key={s.name} value={s.name}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="act-snapnotes-text">Description</label>
                  <textarea
                    id="act-snapnotes-text"
                    rows={3}
                    autoFocus
                    maxLength={500}
                    value={actionNotes}
                    onChange={(e) => setActionNotes(e.target.value)}
                    placeholder="What this snapshot is for"
                  />
                  <p className="field-hint">Leave empty to clear the note.</p>
                </div>
              </>
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
            {action.type === 'drop' && (
              <div className="field">
                <label htmlFor="act-drop">Snapshot</label>
                <select
                  id="act-drop"
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
                <p className="field-hint">Removes this snapshot. The volume's current data stays.</p>
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
              <label htmlFor="vol-notes">Notes</label>
              <textarea
                id="vol-notes"
                rows={3}
                maxLength={500}
                value={vol.notes}
                onChange={(e) => setVol({ ...vol, notes: e.target.value })}
                placeholder="Optional"
              />
            </div>
            <div className="field">
              <label htmlFor="vol-backend">Storage</label>
              <select
                id="vol-backend"
                value={vol.backend}
                onChange={(e) => setVol({ ...vol, backend: e.target.value })}
              >
                <option value="replica">Local (this node)</option>
                <option value="rbd" disabled={!inv.host?.rbd}>
                  Shared (Ceph RBD){inv.host?.rbd ? '' : ' — rbd CLI not found'}
                </option>
                <option value="zfs" disabled={!inv.host?.zfs}>
                  ZFS zvol{inv.host?.zfs ? '' : ' — zfs CLI not found'}
                </option>
              </select>
            </div>
          </form>
        </Modal>
      )}

      {notesEdit && (
        <Modal
          title={`Notes for ${notesEdit.name}`}
          onClose={() => setNotesEdit(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setNotesEdit(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="iso-notes-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          <form id="iso-notes-form" onSubmit={saveIsoNotes}>
            <div className="field">
              <label htmlFor="iso-edit-notes">Notes</label>
              <textarea
                id="iso-edit-notes"
                rows={3}
                autoFocus
                maxLength={500}
                value={editNotes}
                onChange={(e) => setEditNotes(e.target.value)}
                placeholder="What this ISO is for"
              />
              <p className="field-hint">Leave empty to clear the note.</p>
            </div>
          </form>
        </Modal>
      )}

      {isoOpen && (
        <Modal
          title="Import ISO"
          hint="Upload an installer, import a host path, or build a cloud-init seed ISO."
          onClose={() => setIsoOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setIsoOpen(false)}>
                Cancel
              </button>
              <button
                type="submit"
                form="import-iso"
                disabled={
                  busy ||
                  (isoMode === 'file'
                    ? !isoFile
                    : isoMode === 'path'
                      ? !isoPath.trim()
                      : !(ci.hostname.trim() || ci.password || ci.sshKey.trim()))
                }
              >
                {busy ? 'Importing…' : 'Import'}
              </button>
            </>
          }
        >
          <form id="import-iso" onSubmit={importIso}>
            <div className="role-pills" style={{ marginBottom: '1rem' }}>
              {[
                ['file', 'Upload file'],
                ['cloudinit', 'Cloud-init'],
                ['path', 'Host path'],
              ].map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  className={`role-pill${isoMode === id ? ' active' : ''}`}
                  onClick={() => setIsoMode(id)}
                >
                  <strong>{label}</strong>
                </button>
              ))}
            </div>
            {isoMode === 'file' ? (
              <div className="field">
                <label htmlFor="iso-file">ISO file</label>
                <input
                  id="iso-file"
                  type="file"
                  accept=".iso,application/x-cd-image,application/octet-stream"
                  onChange={(e) => setIsoFile(e.target.files?.[0] || null)}
                />
                {isoFile && <p className="muted">{isoFile.name}</p>}
              </div>
            ) : isoMode === 'cloudinit' ? (
              <>
                <p className="muted" style={{ marginTop: 0 }}>
                  NoCloud seed for Ubuntu/Alpine cloud disks. Not a Proxmox/ESXi installer.
                </p>
                <div className="form-grid">
                  <div className="field">
                    <label htmlFor="ci-name">ISO name</label>
                    <input
                      id="ci-name"
                      value={ci.name}
                      onChange={(e) => setCi({ ...ci, name: e.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label htmlFor="ci-host">Hostname</label>
                    <input
                      id="ci-host"
                      value={ci.hostname}
                      onChange={(e) => setCi({ ...ci, hostname: e.target.value })}
                      placeholder="web-1"
                    />
                  </div>
                </div>
                <div className="form-grid">
                  <div className="field">
                    <label htmlFor="ci-user">User</label>
                    <input
                      id="ci-user"
                      value={ci.user}
                      onChange={(e) => setCi({ ...ci, user: e.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label htmlFor="ci-pass">Password</label>
                    <input
                      id="ci-pass"
                      type="password"
                      value={ci.password}
                      onChange={(e) => setCi({ ...ci, password: e.target.value })}
                    />
                  </div>
                </div>
                <div className="field">
                  <label htmlFor="ci-ssh">SSH authorized keys</label>
                  <textarea
                    id="ci-ssh"
                    rows={3}
                    value={ci.sshKey}
                    onChange={(e) => setCi({ ...ci, sshKey: e.target.value })}
                    placeholder="ssh-ed25519 AAAA… (one per line)"
                  />
                </div>
              </>
            ) : (
              <div className="field">
                <label htmlFor="iso-path">Host path</label>
                <input
                  id="iso-path"
                  required={isoMode === 'path'}
                  value={isoPath}
                  onChange={(e) => setIsoPath(e.target.value)}
                  placeholder="/path/to/os.iso"
                />
              </div>
            )}
            <div className="field">
              <label htmlFor="iso-notes">Notes</label>
              <textarea
                id="iso-notes"
                rows={2}
                maxLength={500}
                value={isoNotes}
                onChange={(e) => setIsoNotes(e.target.value)}
                placeholder="Optional — what this ISO is for"
              />
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
