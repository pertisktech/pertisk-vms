import { useOutletContext } from 'react-router-dom'
import { asList, formatBytes, publicIpv6 } from '../api'
import { Icon } from '../components/Icons'

export default function Cluster() {
  const { inv } = useOutletContext()
  const { cluster, error, setError } = inv
  const members = asList(cluster?.members)

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="cluster" size={20} />
            {cluster?.name || 'Cluster'}
          </h1>
          <p className="dash-lead muted">
            {cluster?.name || 'cluster'} · gen {cluster?.generation ?? 0} ·{' '}
            {cluster?.quorum ? 'quorum held' : 'no quorum'}
            {cluster?.fenced ? ' · fenced' : ''}
          </p>
        </div>
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
          const v6 = publicIpv6(m.ipv6)
          return (
            <article key={m.id} className="guest-card">
              <div className="guest-card-top">
                <span className={`guest-orb ${m.online ? 'running' : 'stopped'}`} />
                <strong>{m.name}</strong>
                {m.id === cluster?.self_id && <span className="badge pending">this node</span>}
                {m.id === cluster?.leader_id && <span className="badge ready">leader</span>}
                <span className={`badge ${m.online ? 'online' : 'offline'}`}>
                  {m.online ? 'online' : 'offline'}
                </span>
              </div>
              <div className="guest-meta">
                <span className="mono-inline">{m.peer_url}</span>
              </div>
              {(m.ipv4?.length || v6.length) ? (
                <div className="guest-meta">
                  {m.ipv4?.length > 0 && <span className="mono-inline">{m.ipv4.join(', ')}</span>}
                  {v6.length > 0 && <span className="mono-inline">{v6.join(', ')}</span>}
                </div>
              ) : null}
              <div className="metric-tile-track" style={{ marginTop: '0.75rem' }}>
                <div className="metric-tile-fill usage-bar-cpu" style={{ width: `${cpuPct}%` }} />
              </div>
              <div className="guest-meta" style={{ marginTop: '0.35rem' }}>
                <span>
                  CPU {m.used_vcpus}/{m.cpus}
                </span>
                <span>
                  Mem {formatBytes((m.used_memory_mib || 0) * 1024 * 1024)} used ·{' '}
                  {formatBytes(
                    Math.max(0, (m.memory_mib || 0) - (m.used_memory_mib || 0)) * 1024 * 1024,
                  )}{' '}
                  free · {formatBytes((m.memory_mib || 0) * 1024 * 1024)} total · {memPct}%
                </span>
              </div>
            </article>
          )
        })}
      </div>
    </div>
  )
}
