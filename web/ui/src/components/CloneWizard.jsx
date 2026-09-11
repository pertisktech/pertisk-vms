import { useState } from 'react'
import { api, guestMemoryBudgetMib, nextVmId } from '../api'
import Modal from './Modal'

export default function CloneWizard({ source, vms, networks, cluster, onClose, onCreated }) {
  const budget = guestMemoryBudgetMib(cluster)
  const [form, setForm] = useState(() => {
    const wanted = Number(source?.spec?.memory_mib) || 1024
    const memory = budget && budget >= 64 ? Math.min(wanted, budget) : wanted
    return {
      id: nextVmId(vms),
      name: source?.spec?.name ? `${source.spec.name}-1` : '',
      vcpus: source?.spec?.vcpus || 1,
      memory_mib: memory,
      ha: true,
      autostart: false,
      linked: true,
      networkId: source?.spec?.nets?.[0]?.network_id || networks[0]?.id || '',
      nicIp: '',
      cloudInit: true,
      ciUser: 'ubuntu',
      ciPassword: '',
      ciSshKey: '',
      start: false,
    }
  })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  function set(patch) {
    setForm((f) => ({ ...f, ...patch }))
  }

  async function submit(e) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await api(`/v1/vms/${source.id}/clone`, {
        method: 'POST',
        body: {
          id: Number(form.id),
          name: form.name.trim(),
          linked: form.linked,
          vcpus: Number(form.vcpus),
          memory_mib: Number(form.memory_mib),
          ha: form.ha,
          autostart: form.autostart,
          network_id: form.networkId || undefined,
          ip: form.nicIp.trim() || undefined,
          cloud_init: form.cloudInit
            ? {
                hostname: form.name.trim(),
                user: form.ciUser.trim() || 'ubuntu',
                password: form.ciPassword || undefined,
                ssh_authorized_keys: form.ciSshKey
                  .split('\n')
                  .map((s) => s.trim())
                  .filter(Boolean),
              }
            : undefined,
          start: form.start,
        },
      })
      await onCreated()
      onClose()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const selectedNetwork = networks.find((n) => n.id === form.networkId)
  const canSubmit = /^\d{3,10}$/.test(form.id) && form.name.trim().length > 0

  return (
    <Modal
      title={`Clone ${source?.spec?.name || source?.id || 'template'}`}
      hint="Clones the template disk, injects a cloud-init seed, and defines a new guest."
      onClose={onClose}
      footer={
        <>
          <button type="button" className="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" form="clone-wizard" disabled={busy || !canSubmit}>
            {busy ? 'Cloning…' : form.start ? 'Clone and start' : 'Clone'}
          </button>
        </>
      }
    >
      {error && <div className="error">{error}</div>}
      <form id="clone-wizard" onSubmit={submit}>
        <div className="form-grid">
          <div className="field">
            <label htmlFor="clone-id">VM ID</label>
            <input
              id="clone-id"
              required
              inputMode="numeric"
              pattern="[0-9]{3,10}"
              maxLength="10"
              value={form.id}
              onChange={(e) => set({ id: e.target.value.replace(/\D/g, '') })}
            />
          </div>
          <div className="field">
            <label htmlFor="clone-name">Name</label>
            <input
              id="clone-name"
              required
              autoFocus
              value={form.name}
              onChange={(e) => set({ name: e.target.value })}
            />
          </div>
        </div>
        <div className="form-grid">
          <div className="field">
            <label htmlFor="clone-cpu">vCPU</label>
            <input
              id="clone-cpu"
              type="number"
              min="1"
              value={form.vcpus}
              onChange={(e) => set({ vcpus: e.target.value })}
            />
          </div>
          <div className="field">
            <label htmlFor="clone-mem">Memory (MiB)</label>
            <input
              id="clone-mem"
              type="number"
              min="64"
              step="64"
              value={form.memory_mib}
              onChange={(e) => set({ memory_mib: e.target.value })}
            />
            {budget != null && (
              <p className="field-hint">
                This node can start guests up to {budget} MiB. Clone still works if you ask for more; start later after lowering memory.
              </p>
            )}
          </div>
        </div>
        <div className="field">
          <label htmlFor="clone-net">Network</label>
          <select id="clone-net" value={form.networkId} onChange={(e) => set({ networkId: e.target.value })}>
            <option value="">None</option>
            {networks.map((n) => (
              <option key={n.id} value={n.id}>
                {n.name} ({n.cidr})
              </option>
            ))}
          </select>
        </div>
        {form.networkId && (
          <div className="field">
            <label htmlFor="clone-ip">Static IP</label>
            <input
              id="clone-ip"
              value={form.nicIp}
              onChange={(e) => set({ nicIp: e.target.value })}
              placeholder={
                selectedNetwork?.gateway
                  ? `DHCP; gateway ${selectedNetwork.gateway} is reserved`
                  : 'leave empty for DHCP'
              }
            />
          </div>
        )}
        <div className="wizard-options">
          <label className="chk">
            <input type="checkbox" checked={form.linked} onChange={(e) => set({ linked: e.target.checked })} />
            <span className="chk-box" />
            <span className="chk-label">
              Linked clone
              <small>qcow2 backing file; falls back to a full copy without qemu-img</small>
            </span>
          </label>
          <label className="chk">
            <input type="checkbox" checked={form.ha} onChange={(e) => set({ ha: e.target.checked })} />
            <span className="chk-box" />
            <span className="chk-label">
              Restart on another node if this one is lost
              <small>High availability</small>
            </span>
          </label>
          <label className="chk">
            <input
              type="checkbox"
              checked={form.autostart}
              onChange={(e) => set({ autostart: e.target.checked })}
            />
            <span className="chk-box" />
            <span className="chk-label">
              Start at boot
              <small>Power on when this node starts</small>
            </span>
          </label>
          <label className="chk">
            <input
              type="checkbox"
              checked={form.cloudInit}
              onChange={(e) => set({ cloudInit: e.target.checked })}
            />
            <span className="chk-box" />
            <span className="chk-label">
              Cloud-init seed
              <small>Hostname, user, password, and SSH keys for a cloud image</small>
            </span>
          </label>
          <label className="chk">
            <input type="checkbox" checked={form.start} onChange={(e) => set({ start: e.target.checked })} />
            <span className="chk-box" />
            <span className="chk-label">
              Start after clone
              <small>Boot the guest as soon as it is defined. Needs {form.memory_mib} MiB free for guests.</small>
            </span>
          </label>
        </div>
        {form.cloudInit && (
          <>
            <div className="form-grid" style={{ marginTop: '1rem' }}>
              <div className="field">
                <label htmlFor="clone-ci-user">User</label>
                <input
                  id="clone-ci-user"
                  value={form.ciUser}
                  onChange={(e) => set({ ciUser: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="clone-ci-pass">Password</label>
                <input
                  id="clone-ci-pass"
                  type="password"
                  value={form.ciPassword}
                  onChange={(e) => set({ ciPassword: e.target.value })}
                />
              </div>
            </div>
            <div className="field">
              <label htmlFor="clone-ci-ssh">SSH authorized keys</label>
              <textarea
                id="clone-ci-ssh"
                rows={3}
                value={form.ciSshKey}
                onChange={(e) => set({ ciSshKey: e.target.value })}
                placeholder="ssh-ed25519 AAAA… (one per line)"
              />
            </div>
          </>
        )}
      </form>
    </Modal>
  )
}
