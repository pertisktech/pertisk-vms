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
  const [error, setError] = useState('')

  if (!canWrite) return null

  function peerUrl() {
    let peer = form.peer.trim().replace(/\/+$/, '')
    if (peer && !/^https?:\/\//i.test(peer)) {
      peer = `https://${peer}`
    }
    return peer
  }

  async function join(e) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await inv.mutate(() =>
        api('/v1/cluster/join', {
          method: 'POST',
          body: {
            peer: peerUrl(),
            username: form.username.trim(),
            password: form.password,
          },
        }),
      )
      setOpen(false)
      setForm({ peer: '', username: 'admin', password: '' })
    } catch (err) {
      setError(err.message || String(err))
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
            hint="HTTPS is fine (self-signed). This node joins the cluster at the peer URL."
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
              {error && <div className="banner danger">{error}</div>}
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
