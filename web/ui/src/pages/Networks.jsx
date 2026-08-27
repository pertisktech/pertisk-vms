import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, netsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'
import { useInventory } from '../useInventory'

const EMPTY = {
  name: '',
  cidr: '10.90.0.0/24',
  gateway: '',
  bridge: '',
  dhcp: true,
  isolate: true,
}

export default function Networks() {
  const { canWrite } = useOutletContext()
  const { networks, vms, error, setError, mutate } = useInventory()
  const confirm = useConfirm()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY)
  const [busy, setBusy] = useState(false)

  function guestsOn(netId) {
    return vms.filter((vm) => netsOf(vm).some((n) => n.network_id === netId)).length
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
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="network" size={20} />
            Networks
          </h1>
          <p className="dash-lead muted">Guest networks and DHCP pools on this node.</p>
        </div>
        {canWrite && (
          <Btn icon="plus" onClick={() => setOpen(true)}>
            Create network
          </Btn>
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
        {networks.length === 0 ? (
          <p className="muted">No networks yet.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
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
                    <td>{n.name}</td>
                    <td className="mono-inline">{n.cidr}</td>
                    <td className="mono-inline">{n.gateway || '—'}</td>
                    <td className="mono-inline">{n.bridge || '—'}</td>
                    <td>
                      <span className={`badge ${n.dhcp !== false ? 'ready' : 'unknown'}`}>
                        {n.dhcp !== false ? 'on' : 'off'}
                      </span>
                    </td>
                    <td>
                      <span className={`badge ${n.isolate !== false ? 'pending' : 'unknown'}`}>
                        {n.isolate !== false ? 'yes' : 'no'}
                      </span>
                    </td>
                    <td>{guestsOn(n.id)}</td>
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
          hint="DHCP assigns addresses from the CIDR. Isolation keeps guests from seeing each other on the bridge."
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
            <div className="field">
              <label htmlFor="net-bridge">Bridge</label>
              <input
                id="net-bridge"
                value={form.bridge}
                onChange={(e) => setForm({ ...form, bridge: e.target.value })}
                placeholder="vmbr0 (optional)"
              />
            </div>
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
          </form>
        </Modal>
      )}
    </div>
  )
}
