import { useState } from 'react'
import { api, parseSize } from '../api'
import Modal from './Modal'

const STEPS = [
  { id: 'guest', label: 'Guest' },
  { id: 'disk', label: 'Disk' },
  { id: 'media', label: 'Media' },
  { id: 'net', label: 'Network' },
  { id: 'review', label: 'Review' },
]

const EMPTY = {
  name: '',
  vcpus: 1,
  memory_mib: 512,
  ha: true,
  diskMode: 'new',
  diskName: '',
  diskSize: '8G',
  volumeId: '',
  iso: '',
  networkId: '',
  start: true,
}

export default function GuestWizard({ volumes, isos, networks, onClose, onCreated }) {
  const [step, setStep] = useState(0)
  const [form, setForm] = useState(EMPTY)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  function set(patch) {
    setForm((f) => ({ ...f, ...patch }))
  }

  function canNext() {
    if (step === 0) return form.name.trim().length > 0 && Number(form.vcpus) >= 1 && Number(form.memory_mib) >= 64
    if (step === 1 && form.diskMode === 'new') return (form.diskName.trim() || form.name.trim()).length > 0
    if (step === 1 && form.diskMode === 'existing') return Boolean(form.volumeId)
    return true
  }

  async function finish(e) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      let volumeId = form.diskMode === 'existing' ? form.volumeId : ''
      if (form.diskMode === 'new') {
        const vol = await api('/v1/volumes', {
          method: 'POST',
          body: {
            name: (form.diskName.trim() || `${form.name.trim()}-disk`),
            size_bytes: parseSize(form.diskSize),
            format: 'raw',
          },
        })
        volumeId = vol.id
      }
      const vm = await api('/v1/vms', {
        method: 'POST',
        body: {
          name: form.name.trim(),
          vcpus: Number(form.vcpus),
          memory_mib: Number(form.memory_mib),
          ha: form.ha,
        },
      })
      if (volumeId) {
        await api(`/v1/vms/${vm.id}/disks`, { method: 'POST', body: { volume_id: volumeId } })
      }
      if (form.iso) {
        await api(`/v1/vms/${vm.id}/cdrom`, { method: 'POST', body: { iso: form.iso } })
      }
      if (form.networkId) {
        await api(`/v1/vms/${vm.id}/nics`, { method: 'POST', body: { network_id: form.networkId } })
      }
      if (form.start) {
        await api(`/v1/vms/${vm.id}/start`, { method: 'POST' })
      }
      await onCreated()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const diskLabel =
    form.diskMode === 'none'
      ? 'No disk'
      : form.diskMode === 'new'
        ? `New ${(form.diskName || `${form.name}-disk`).trim()} (${form.diskSize})`
        : volumes.find((v) => v.id === form.volumeId)?.name || 'Existing volume'
  const isoLabel = form.iso || 'None'
  const netLabel = networks.find((n) => n.id === form.networkId)?.name || 'None'

  return (
    <Modal
      title="Create guest"
      hint="Name the machine, then optionally attach a disk, ISO, and network."
      wizard
      onClose={onClose}
      footer={
        <div className="wizard-footer">
          <button type="button" className="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <div className="wizard-footer-right">
            {step > 0 && (
              <button type="button" className="secondary" onClick={() => setStep((s) => s - 1)} disabled={busy}>
                Back
              </button>
            )}
            {step < STEPS.length - 1 ? (
              <button type="button" onClick={() => setStep((s) => s + 1)} disabled={!canNext()}>
                Next
              </button>
            ) : (
              <button type="submit" form="guest-wizard" disabled={busy || !canNext()}>
                {busy ? 'Creating…' : form.start ? 'Create and start' : 'Create'}
              </button>
            )}
          </div>
        </div>
      }
    >
      <div className="wizard-steps">
        {STEPS.map((item, i) => (
          <button
            key={item.id}
            type="button"
            className={`wizard-step${i === step ? ' current' : ''}${i < step ? ' done' : ''}`}
            onClick={() => i <= step && setStep(i)}
            disabled={i > step}
          >
            <span className="wizard-step-num">{i + 1}</span>
            <span className="wizard-step-label">{item.label}</span>
          </button>
        ))}
      </div>

      {error && <div className="error">{error}</div>}

      <form id="guest-wizard" onSubmit={finish}>
        {step === 0 && (
          <>
            <p className="wizard-section-title">Machine</p>
            <div className="field">
              <label htmlFor="guest-name">Name</label>
              <input
                id="guest-name"
                required
                autoFocus
                value={form.name}
                onChange={(e) => set({ name: e.target.value })}
                placeholder="web-1"
              />
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="guest-cpu">vCPU</label>
                <input
                  id="guest-cpu"
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => set({ vcpus: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="guest-mem">Memory MiB</label>
                <input
                  id="guest-mem"
                  type="number"
                  min="64"
                  value={form.memory_mib}
                  onChange={(e) => set({ memory_mib: e.target.value })}
                />
              </div>
            </div>
            <label className="chk">
              <input type="checkbox" checked={form.ha} onChange={(e) => set({ ha: e.target.checked })} />
              <span className="chk-box" />
              <span className="chk-label">Restart elsewhere if this node is lost</span>
            </label>
          </>
        )}

        {step === 1 && (
          <>
            <p className="wizard-section-title">Boot disk</p>
            <div className="role-pills">
              {[
                ['new', 'New volume'],
                ['existing', 'Existing'],
                ['none', 'None'],
              ].map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  className={`role-pill${form.diskMode === id ? ' active' : ''}`}
                  onClick={() => set({ diskMode: id })}
                >
                  <strong>{label}</strong>
                </button>
              ))}
            </div>
            {form.diskMode === 'new' && (
              <div className="form-grid" style={{ marginTop: '1rem' }}>
                <div className="field">
                  <label htmlFor="disk-name">Volume name</label>
                  <input
                    id="disk-name"
                    value={form.diskName}
                    placeholder={`${form.name || 'guest'}-disk`}
                    onChange={(e) => set({ diskName: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label htmlFor="disk-size">Size</label>
                  <input
                    id="disk-size"
                    value={form.diskSize}
                    onChange={(e) => set({ diskSize: e.target.value })}
                  />
                </div>
              </div>
            )}
            {form.diskMode === 'existing' && (
              <div className="field" style={{ marginTop: '1rem' }}>
                <label htmlFor="disk-existing">Volume</label>
                <select
                  id="disk-existing"
                  value={form.volumeId}
                  onChange={(e) => set({ volumeId: e.target.value })}
                >
                  <option value="">Select…</option>
                  {volumes.map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.name}
                    </option>
                  ))}
                </select>
                {volumes.length === 0 && <p className="muted">No volumes yet. Create one in Storage, or pick New volume.</p>}
              </div>
            )}
          </>
        )}

        {step === 2 && (
          <>
            <p className="wizard-section-title">Install media</p>
            <div className="field">
              <label htmlFor="guest-iso">ISO</label>
              <select id="guest-iso" value={form.iso} onChange={(e) => set({ iso: e.target.value })}>
                <option value="">None</option>
                {isos.map((iso) => (
                  <option key={iso.name} value={iso.name}>
                    {iso.name}
                  </option>
                ))}
              </select>
              {isos.length === 0 && (
                <p className="muted">Import an ISO under Storage if you want to boot an installer.</p>
              )}
            </div>
          </>
        )}

        {step === 3 && (
          <>
            <p className="wizard-section-title">Network</p>
            <div className="field">
              <label htmlFor="guest-net">Guest network</label>
              <select
                id="guest-net"
                value={form.networkId}
                onChange={(e) => set({ networkId: e.target.value })}
              >
                <option value="">None</option>
                {networks.map((n) => (
                  <option key={n.id} value={n.id}>
                    {n.name} ({n.cidr})
                  </option>
                ))}
              </select>
            </div>
          </>
        )}

        {step === 4 && (
          <>
            <p className="wizard-section-title">Review</p>
            <dl className="kv">
              <div>
                <dt>Name</dt>
                <dd>{form.name}</dd>
              </div>
              <div>
                <dt>Size</dt>
                <dd>
                  {form.vcpus} vCPU · {form.memory_mib} MiB
                </dd>
              </div>
              <div>
                <dt>Disk</dt>
                <dd>{diskLabel}</dd>
              </div>
              <div>
                <dt>ISO</dt>
                <dd>{isoLabel}</dd>
              </div>
              <div>
                <dt>Network</dt>
                <dd>{netLabel}</dd>
              </div>
            </dl>
            <label className="chk" style={{ marginTop: '1rem' }}>
              <input type="checkbox" checked={form.start} onChange={(e) => set({ start: e.target.checked })} />
              <span className="chk-box" />
              <span className="chk-label">Start after create</span>
            </label>
          </>
        )}
      </form>
    </Modal>
  )
}
