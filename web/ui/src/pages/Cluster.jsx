import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, asList } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import { useInventory } from '../useInventory'

export default function Cluster() {
  const { canWrite } = useOutletContext()
  const { cluster, error, setError, mutate } = useInventory()
  const members = asList(cluster?.members)
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState({ peer: '', username: 'admin', password: '' })
  const [busy, setBusy] = useState(false)

  async function join(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await mutate(() =>
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
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="cluster" size={20} />
            Cluster
          </h1>
          <p className="dash-lead muted">
            {cluster?.name || 'cluster'} · gen {cluster?.generation ?? 0} ·{' '}
            {cluster?.quorum ? 'quorum held' : 'no quorum'}
            {cluster?.fenced ? ' · fenced' : ''}
          </p>
        </div>
        {canWrite && (
          <Btn icon="plus" onClick={() => setOpen(true)}>
            Join peer
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

      <div className="guest-grid">
        {members.map((m) => {
          const cpuPct = m.cpus ? Math.round((m.used_vcpus / m.cpus) * 100) : 0
          const memPct = m.memory_mib ? Math.round((m.used_memory_mib / m.memory_mib) * 100) : 0
          return (
            <article key={m.id} className="guest-card">
              <div className="guest-card-top">
                <span className={`guest-orb ${m.online ? 'running' : 'stopped'}`} />
                <strong>{m.name}</strong>
                <span className={`badge ${m.online ? 'online' : 'offline'}`}>
                  {m.online ? 'online' : 'offline'}
                </span>
              </div>
              <div className="guest-meta">
                <span className="mono-inline">{m.peer_url}</span>
              </div>
              <div className="metric-tile-track" style={{ marginTop: '0.75rem' }}>
                <div className="metric-tile-fill usage-bar-cpu" style={{ width: `${cpuPct}%` }} />
              </div>
              <div className="guest-meta" style={{ marginTop: '0.35rem' }}>
                <span>
                  CPU {m.used_vcpus}/{m.cpus}
                </span>
                <span>
                  Mem {m.used_memory_mib}/{m.memory_mib} MiB · {memPct}%
                </span>
              </div>
            </article>
          )
        })}
      </div>

      {open && (
        <Modal
          title="Join a peer"
          hint="This node will join the cluster advertised at the peer URL."
          onClose={() => setOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="join-peer" disabled={busy}>
                Join
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
                placeholder="http://127.0.0.1:7481"
              />
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor="peer-user">Username</label>
                <input
                  id="peer-user"
                  value={form.username}
                  onChange={(e) => setForm({ ...form, username: e.target.value })}
                />
              </div>
              <div className="field">
                <label htmlFor="peer-pass">Password</label>
                <input
                  id="peer-pass"
                  type="password"
                  value={form.password}
                  onChange={(e) => setForm({ ...form, password: e.target.value })}
                />
              </div>
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
