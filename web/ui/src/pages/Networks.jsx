import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'
import { useInventory } from '../useInventory'

export default function Networks() {
  const { canWrite } = useOutletContext()
  const { networks, error, setError, mutate } = useInventory()
  const confirm = useConfirm()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ name: '', cidr: '10.90.0.0/24' })
  const [busy, setBusy] = useState(false)

  async function createNet(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await mutate(() =>
        api('/v1/networks', {
          method: 'POST',
          body: { name: form.name.trim(), cidr: form.cidr.trim() },
        }),
      )
      setForm({ name: '', cidr: '10.90.0.0/24' })
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
                  <th>Bridge</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {networks.map((n) => (
                  <tr key={n.id}>
                    <td>{n.name}</td>
                    <td className="mono-inline">{n.cidr}</td>
                    <td className="mono-inline">{n.bridge || '—'}</td>
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
              <label htmlFor="net-cidr">CIDR</label>
              <input
                id="net-cidr"
                required
                value={form.cidr}
                onChange={(e) => setForm({ ...form, cidr: e.target.value })}
              />
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
