import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, netsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'

const EMPTY = {
  name: '',
  mode: 'nat',
  cidr: '10.90.0.0/24',
  gateway: '',
  bridge: '',
  dhcp: true,
  isolate: true,
}

export default function Networks() {
  const { canWrite, inv } = useOutletContext()
  const { networks, vms, error, setError, mutate } = inv
  const confirm = useConfirm()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY)
  const [busy, setBusy] = useState(false)

  function guestsOn(netId) {
    return vms.filter((vm) => netsOf(vm).some((n) => n.network_id === netId)).length
  }

  function setMode(mode) {
    if (mode === 'bridge') {
      setForm({
        ...form,
        mode,
        dhcp: false,
        isolate: false,
        cidr: '0.0.0.0/0',
        gateway: '',
        bridge: form.bridge || 'br0',
      })
    } else {
      setForm({
        ...form,
        mode,
        dhcp: true,
        isolate: true,
        cidr: form.cidr === '0.0.0.0/0' ? '10.90.0.0/24' : form.cidr,
      })
    }
  }

  async function createNet(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await mutate(() =>
        api('/v1/networks', {
          method: 'POST',
          body: {
            name: form.name.trim(),
            mode: form.mode,
            cidr: form.cidr.trim(),
            gateway: form.gateway.trim() || undefined,
            bridge: form.bridge.trim() || undefined,
            dhcp: form.dhcp,
            isolate: form.isolate,
          },
        }),
      )
      setForm(EMPTY)
      setOpen(false)
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
          <Btn icon="plus" onClick={() => setOpen(true)}>
            Create network
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
          <h3>Networks</h3>
        </header>
        {networks.length === 0 ? (
          <div className="pve-empty">
            <Icon name="network" size={22} />
            <span className="muted">No networks yet.</span>
          </div>
        ) : (
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Mode</th>
                  <th>CIDR</th>
                  <th>Gateway</th>
                  <th>Bridge</th>
                  <th>DHCP</th>
                  <th>Isolate</th>
                  <th>Guests</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {networks.map((n) => (
                  <tr key={n.id}>
                    <td>
                      <strong>{n.name}</strong>
                    </td>
                    <td className="mono-inline muted">{n.mode || 'nat'}</td>
                    <td className="mono-inline muted">{n.cidr}</td>
                    <td className="mono-inline muted">{n.gateway || '—'}</td>
                    <td className="mono-inline muted">{n.bridge || '—'}</td>
                    <td>
                      <span className={`pve-pill${n.dhcp !== false ? ' ok' : ''}`}>
                        {n.dhcp !== false ? 'On' : 'Off'}
                      </span>
                    </td>
                    <td>
                      <span className="pve-pill">{n.isolate !== false ? 'Yes' : 'No'}</span>
                    </td>
                    <td className="mono-inline muted">{guestsOn(n.id)}</td>
                    {canWrite && (
                      <td className="col-actions">
                        <Btn
                          icon="trash"
                          variant="danger"
                          onClick={async () => {
                            const ok = await confirm({
                              title: 'Delete network',
                              message: `Delete ${n.name}?`,
                              confirmLabel: 'Delete',
                            })
                            if (ok) mutate(() => api(`/v1/networks/${n.id}`, { method: 'DELETE' }))
                          }}
                        >
                          Delete
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

      {open && (
        <Modal
          title="Create network"
          hint={
            form.mode === 'bridge'
              ? 'Bridge mode attaches guest TAPs to an existing host bridge (LAN DHCP). No CIDR overlap check.'
              : 'NAT mode creates an isolated bridge and DHCP pool. CIDR must not overlap the host LAN.'
          }
          onClose={() => setOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="create-net" disabled={busy}>
                Create
              </button>
            </>
          }
        >
          <form id="create-net" onSubmit={createNet}>
            <div className="field">
              <label htmlFor="net-name">Name</label>
              <input
                id="net-name"
                required
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="net-mode">Mode</label>
              <select id="net-mode" value={form.mode} onChange={(e) => setMode(e.target.value)}>
                <option value="nat">NAT (isolated)</option>
                <option value="bridge">Bridge (existing LAN)</option>
              </select>
            </div>
            {form.mode === 'nat' && (
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="net-cidr">CIDR</label>
                  <input
                    id="net-cidr"
                    required
                    value={form.cidr}
                    onChange={(e) => setForm({ ...form, cidr: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label htmlFor="net-gw">Gateway</label>
                  <input
                    id="net-gw"
                    value={form.gateway}
                    onChange={(e) => setForm({ ...form, gateway: e.target.value })}
                    placeholder="optional"
                  />
                </div>
              </div>
            )}
            <div className="field">
              <label htmlFor="net-bridge">Bridge</label>
              <input
                id="net-bridge"
                required={form.mode === 'bridge'}
                value={form.bridge}
                onChange={(e) => setForm({ ...form, bridge: e.target.value })}
                placeholder={form.mode === 'bridge' ? 'br0' : 'vmbr0 (optional)'}
              />
            </div>
            {form.mode === 'nat' && (
              <>
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={form.dhcp}
                    onChange={(e) => setForm({ ...form, dhcp: e.target.checked })}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">DHCP pool from this CIDR</span>
                </label>
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={form.isolate}
                    onChange={(e) => setForm({ ...form, isolate: e.target.checked })}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">Isolate guests on this bridge</span>
                </label>
              </>
            )}
          </form>
        </Modal>
      )}
    </div>
  )
}
