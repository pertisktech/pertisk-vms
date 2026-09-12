import { useState } from 'react'
import { createPortal } from 'react-dom'
import { api, asList } from '../api'
import { Btn } from './Icons'
import Modal from './Modal'
import { useConfirm } from './Confirm'

export default function JoinCluster({ canWrite, inv }) {
  const cluster = inv?.cluster
  const members = asList(cluster?.members)
  const confirm = useConfirm()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ peer: '', username: 'admin', password: '' })
  const [busy, setBusy] = useState(false)

  if (!canWrite) return null

  async function join(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await inv.mutate(() =>
        api('/v1/cluster/join', {
          method: 'POST',
          body: {
            peer: form.peer.trim(),
            username: form.username.trim(),
            password: form.password,
          },
        }),
      )
      setOpen(false)
      setForm({ peer: '', username: 'admin', password: '' })
    } catch {
      /* inventory */
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      {members.length > 1 && (
        <Btn
          icon="logout"
          variant="danger"
          onClick={async () => {
            const ok = await confirm({
              title: 'Leave cluster',
              message:
                'This node becomes a solo cluster. Guests on other nodes stay there. Continue?',
              confirmLabel: 'Leave',
            })
            if (ok) inv.mutate(() => api('/v1/cluster/leave', { method: 'POST' }))
          }}
        >
          Leave
        </Btn>
      )}
      <Btn icon="cluster" onClick={() => setOpen(true)}>
        Join cluster
      </Btn>

      {open &&
        createPortal(
          <Modal
            title="Join a cluster"
            hint="This node joins the cluster advertised at the peer URL."
            onClose={() => setOpen(false)}
            footer={
              <>
                <button type="button" className="secondary" onClick={() => setOpen(false)}>
                  Cancel
                </button>
                <button type="submit" form="join-peer" disabled={busy || !form.peer.trim()}>
                  {busy ? 'Joining…' : 'Join'}
                </button>
              </>
            }
          >
            <form id="join-peer" onSubmit={join}>
              <div className="field">
                <label htmlFor="peer-url">Peer URL</label>
                <input
                  id="peer-url"
                  required
                  value={form.peer}
                  onChange={(e) => setForm({ ...form, peer: e.target.value })}
                  placeholder="https://10.1.1.10:7443"
                  autoFocus
                />
              </div>
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="peer-user">Username</label>
                  <input
                    id="peer-user"
                    value={form.username}
                    onChange={(e) => setForm({ ...form, username: e.target.value })}
                    autoComplete="username"
                  />
                </div>
                <div className="field">
                  <label htmlFor="peer-pass">Password</label>
                  <input
                    id="peer-pass"
                    type="password"
                    value={form.password}
                    onChange={(e) => setForm({ ...form, password: e.target.value })}
                    autoComplete="current-password"
                  />
                </div>
              </div>
            </form>
          </Modal>,
          document.body,
        )}
    </>
  )
}
