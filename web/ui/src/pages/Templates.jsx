import { useState } from 'react'
import { Link, useNavigate, useOutletContext } from 'react-router-dom'
import { api, disksOf, formatBytes, isTemplate } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import CloneWizard from '../components/CloneWizard'
import { useConfirm } from '../components/Confirm'

function imageFormat(name) {
  const lower = (name || '').toLowerCase()
  if (lower.endsWith('.qcow2') || lower.endsWith('.img')) return 'qcow2'
  return 'raw'
}

export default function Templates() {
  const { canWrite, inv } = useOutletContext()
  const nav = useNavigate()
  const { vms, volumes, networks, cluster, host, error, setError, mutate, refresh } = inv
  const confirm = useConfirm()
  const templates = vms.filter(isTemplate)
  const freeVolumes = volumes.filter((vol) => {
    const used = vms.some((vm) => disksOf(vm).some((d) => d.volume_id === vol.id))
    return !used
  })

  const [importOpen, setImportOpen] = useState(false)
  const [fromVolOpen, setFromVolOpen] = useState(false)
  const [cloneOf, setCloneOf] = useState(null)
  const [busy, setBusy] = useState(false)
  const [file, setFile] = useState(null)
  const [form, setForm] = useState({ name: '', vcpus: 1, memory_mib: 1024, format: 'qcow2', notes: '' })
  const [volForm, setVolForm] = useState({
    name: '',
    volume_id: '',
    vcpus: 1,
    memory_mib: 1024,
    notes: '',
  })
  const [importError, setImportError] = useState('')
  const [notesEdit, setNotesEdit] = useState(null)
  const [editNotes, setEditNotes] = useState('')

  const takenNames = new Set((vms || []).map((vm) => vm.spec?.name).filter(Boolean))
  const importName = form.name.trim()
  const importNameTaken = importName && takenNames.has(importName)
  let importAs = importName
  if (importNameTaken) {
    importAs = `${importName}-2`
    for (let i = 2; i < 10_000; i += 1) {
      const next = `${importName}-${i}`
      if (!takenNames.has(next)) {
        importAs = next
        break
      }
    }
  }

  async function importImage(e) {
    e.preventDefault()
    if (!file) return
    setBusy(true)
    setImportError('')
    try {
      const name = form.name.trim() || file.name.replace(/\.[^.]+$/, '')
      const format = form.format || imageFormat(file.name)
      const qs = new URLSearchParams({
        name,
        format,
        vcpus: String(Number(form.vcpus) || 1),
        memory_mib: String(Number(form.memory_mib) || 1024),
      })
      const notes = form.notes.trim()
      if (notes) qs.set('notes', notes)
      await mutate(() =>
        api(`/v1/templates/import?${qs}`, {
          method: 'POST',
          headers: { 'content-type': 'application/octet-stream' },
          body: file,
        }),
      )
      setFile(null)
      setForm({ name: '', vcpus: 1, memory_mib: 1024, format: 'qcow2', notes: '' })
      setImportOpen(false)
    } catch (err) {
      setImportError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function createFromVolume(e) {
    e.preventDefault()
    setBusy(true)
    try {
      const notes = volForm.notes.trim()
      await mutate(() =>
        api('/v1/templates', {
          method: 'POST',
          body: {
            name: volForm.name.trim(),
            volume_id: volForm.volume_id,
            vcpus: Number(volForm.vcpus) || 1,
            memory_mib: Number(volForm.memory_mib) || 1024,
            ...(notes ? { notes } : {}),
          },
        }),
      )
      setVolForm({ name: '', volume_id: '', vcpus: 1, memory_mib: 1024, notes: '' })
      setFromVolOpen(false)
    } catch {
      /* inventory */
    } finally {
      setBusy(false)
    }
  }

  function openNotes(vm) {
    setNotesEdit(vm)
    setEditNotes(vm.spec?.notes || '')
  }

  async function saveNotes(e) {
    e.preventDefault()
    if (!notesEdit) return
    setBusy(true)
    try {
      await mutate(() =>
        api(`/v1/vms/${notesEdit.id}`, {
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

  return (
    <div className="pve-tab-page">
      {canWrite && (
        <div className="pve-action-row">
          <Btn icon="disk" variant="secondary" onClick={() => setFromVolOpen(true)} disabled={freeVolumes.length === 0}>
            From volume
          </Btn>
          <Btn icon="plus" onClick={() => setImportOpen(true)}>
            Import image
          </Btn>
        </div>
      )}
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
          <h3>Templates</h3>
        </header>
        {templates.length === 0 ? (
          <div className="pve-empty">
            <Icon name="template" size={22} />
            <span className="muted">
              No templates yet. Import an Ubuntu/Debian cloud image (qcow2), or convert a stopped guest from Options.
            </span>
          </div>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>ID</th>
                  <th>vCPU</th>
                  <th>Memory</th>
                  <th>Disks</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {templates.map((vm) => (
                  <tr key={vm.id}>
                    <td>
                      <Link to={`/vm/${vm.id}/summary`} className="pve-link">
                        <Icon name="template" size={14} />
                        <strong>{vm.spec?.name || vm.id}</strong>
                      </Link>
                      {vm.spec?.notes && <div className="muted">{vm.spec.notes}</div>}
                    </td>
                    <td className="mono-inline muted">{vm.id}</td>
                    <td className="mono-inline muted">{vm.spec?.vcpus || 1}</td>
                    <td className="mono-inline muted">{vm.spec?.memory_mib || 0} MiB</td>
                    <td className="mono-inline muted">
                      {disksOf(vm)
                        .filter((d) => !d.cdrom)
                        .map((d) => volumes.find((v) => v.id === d.volume_id)?.name || 'disk')
                        .join(', ') || '—'}
                    </td>
                    {canWrite && (
                      <td className="col-actions">
                        <div className="row-actions">
                          <Btn icon="clone" variant="secondary" onClick={() => setCloneOf(vm)}>
                            Clone
                          </Btn>
                          <Btn variant="secondary" onClick={() => openNotes(vm)}>
                            Notes
                          </Btn>
                          <Btn
                            icon="trash"
                            variant="danger"
                            onClick={async () => {
                              const ok = await confirm({
                                title: 'Destroy template',
                                message: `Remove ${vm.spec?.name || vm.id}? Linked clones keep their backing disk.`,
                                confirmLabel: 'Destroy',
                              })
                              if (ok) mutate(() => api(`/v1/vms/${vm.id}`, { method: 'DELETE' }))
                            }}
                          >
                            Destroy
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

      {notesEdit && (
        <Modal
          title={`Notes for ${notesEdit.spec?.name || notesEdit.id}`}
          onClose={() => setNotesEdit(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setNotesEdit(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="tpl-notes-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          <form id="tpl-notes-form" onSubmit={saveNotes}>
            <div className="field">
              <label htmlFor="tpl-edit-notes">Notes</label>
              <textarea
                id="tpl-edit-notes"
                rows={3}
                autoFocus
                maxLength={500}
                value={editNotes}
                onChange={(e) => setEditNotes(e.target.value)}
                placeholder="What this template is for"
              />
              <p className="field-hint">Leave empty to clear the note. Clones do not inherit it.</p>
            </div>
          </form>
        </Modal>
      )}

      {importOpen && (
        <Modal
          title="Import cloud image"
          hint="Upload a cloud disk (Ubuntu cloudimg, AlmaLinux GenericCloud qcow2). Not an installer ISO, and not a cidata seed."
          onClose={() => {
            setImportOpen(false)
            setImportError('')
          }}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setImportOpen(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="tpl-import" disabled={busy || !file}>
                {busy ? 'Importing…' : 'Import'}
              </button>
            </>
          }
        >
          <form id="tpl-import" onSubmit={importImage}>
            {importError && <div className="error">{importError}</div>}
            <div className="field">
              <label htmlFor="tpl-file">Image file</label>
              <input
                id="tpl-file"
                type="file"
                accept=".qcow2,.img,.raw"
                onChange={(e) => {
                  const next = e.target.files?.[0] || null
                  setFile(next)
                  if (next) {
                    setForm((f) => ({
                      ...f,
                      name: f.name || next.name.replace(/\.[^.]+$/, ''),
                      format: imageFormat(next.name),
                    }))
                  }
                }}
              />
              {file?.name?.toLowerCase().endsWith('.iso') && (
                <p className="error">
                  This looks like an installer ISO. Cloud templates need a GenericCloud qcow2 disk image.
                </p>
              )}
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="tpl-name">Template name</label>
                <input
                  id="tpl-name"
                  required
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                  placeholder="ubuntu-24.04"
                />
                {importNameTaken && (
                  <p className="field-hint">
                    <code>{importName}</code> already exists (guest or template). Import will be saved as{' '}
                    <code>{importAs}</code>.
                  </p>
                )}
              </div>
              <div className="field">
                <label htmlFor="tpl-fmt">Format</label>
                <select
                  id="tpl-fmt"
                  value={form.format}
                  onChange={(e) => setForm({ ...form, format: e.target.value })}
                >
                  <option value="qcow2">qcow2</option>
                  <option value="raw">raw</option>
                </select>
              </div>
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="tpl-cpu">Default vCPU</label>
                <input
                  id="tpl-cpu"
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => setForm({ ...form, vcpus: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="tpl-mem">Default memory (MiB)</label>
                <input
                  id="tpl-mem"
                  type="number"
                  min="64"
                  step="64"
                  value={form.memory_mib}
                  onChange={(e) => setForm({ ...form, memory_mib: e.target.value })}
                />
                <p className="field-hint">Used when you clone and start a guest. Import does not consume this RAM.</p>
              </div>
            </div>
            <div className="field">
              <label htmlFor="tpl-notes">Notes</label>
              <textarea
                id="tpl-notes"
                rows={2}
                maxLength={500}
                value={form.notes}
                onChange={(e) => setForm({ ...form, notes: e.target.value })}
                placeholder="Optional — what this template is for"
              />
            </div>
          </form>
        </Modal>
      )}

      {fromVolOpen && (
        <Modal
          title="Template from volume"
          hint="Wrap an unused imported disk as a cloud template."
          onClose={() => setFromVolOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setFromVolOpen(false)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="tpl-vol" disabled={busy || !volForm.name.trim() || !volForm.volume_id}>
                {busy ? 'Creating…' : 'Create'}
              </button>
            </>
          }
        >
          <form id="tpl-vol" onSubmit={createFromVolume}>
            <div className="field">
              <label htmlFor="tpl-vol-name">Name</label>
              <input
                id="tpl-vol-name"
                required
                value={volForm.name}
                onChange={(e) => setVolForm({ ...volForm, name: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="tpl-vol-id">Volume</label>
              <select
                id="tpl-vol-id"
                value={volForm.volume_id}
                onChange={(e) => setVolForm({ ...volForm, volume_id: e.target.value })}
              >
                <option value="">Select…</option>
                {freeVolumes.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.name} ({formatBytes(v.size_bytes)})
                  </option>
                ))}
              </select>
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="tpl-vol-cpu">vCPU</label>
                <input
                  id="tpl-vol-cpu"
                  type="number"
                  min="1"
                  value={volForm.vcpus}
                  onChange={(e) => setVolForm({ ...volForm, vcpus: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="tpl-vol-mem">Memory (MiB)</label>
                <input
                  id="tpl-vol-mem"
                  type="number"
                  min="64"
                  step="64"
                  value={volForm.memory_mib}
                  onChange={(e) => setVolForm({ ...volForm, memory_mib: e.target.value })}
                />
                <p className="field-hint">Used when you clone and start a guest. Import does not consume this RAM.</p>
              </div>
            </div>
            <div className="field">
              <label htmlFor="tpl-vol-notes">Notes</label>
              <textarea
                id="tpl-vol-notes"
                rows={2}
                maxLength={500}
                value={volForm.notes}
                onChange={(e) => setVolForm({ ...volForm, notes: e.target.value })}
                placeholder="Optional — what this template is for"
              />
            </div>
          </form>
        </Modal>
      )}

      {cloneOf && (
        <CloneWizard
          source={cloneOf}
          vms={vms}
          volumes={volumes}
          networks={networks}
          cluster={cluster}
          host={host}
          onClose={() => setCloneOf(null)}
          onCreated={async (vmId) => {
            await refresh()
            setCloneOf(null)
            if (vmId != null && vmId !== '') nav(`/vm/${vmId}/console`)
          }}
        />
      )}
    </div>
  )
}
