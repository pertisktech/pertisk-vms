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
            Cluster
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

      <div className="cluster-grid">
        {members.map((m) => {
          const cpuPct = m.cpus ? Math.round((m.used_vcpus / m.cpus) * 100) : 0
          const memPct = m.memory_mib ? Math.round((m.used_memory_mib / m.memory_mib) * 100) : 0
          const v6 = publicIpv6(m.ipv6)
          return (
            <article key={m.id} className="cluster-card">
              <div className="cluster-card-head">
                <span className={`guest-orb ${m.online ? 'running' : 'stopped'}`} />
                <strong className="cluster-card-name" title={m.name}>
                  {m.name || m.id}
                </strong>
              </div>
              <div className="cluster-card-badges">
                {m.id === cluster?.self_id && <span className="badge pending">this node</span>}
                {m.id === cluster?.leader_id && <span className="badge ready">leader</span>}
                <span className={`badge ${m.online ? 'online' : 'offline'}`}>
                  {m.online ? 'online' : 'offline'}
                </span>
              </div>
              {(m.ipv4?.length || v6.length) ? (
                <div className="cluster-card-addrs" title={[...(m.ipv4 || []), ...v6].join(', ')}>
                  {m.ipv4?.length > 0 && <span>{m.ipv4[0]}</span>}
                  {v6.length > 0 && <span>{v6[0]}</span>}
                </div>
              ) : (
                <div className="cluster-card-addrs muted">{m.peer_url}</div>
              )}
              <div className="metric-tile-track cluster-card-bar">
                <div className="metric-tile-fill usage-bar-cpu" style={{ width: `${cpuPct}%` }} />
              </div>
              <div className="cluster-card-usage">
                <span>
                  CPU {m.used_vcpus}/{m.cpus}
                </span>
                <span>
                  {formatBytes((m.used_memory_mib || 0) * 1024 * 1024)} /{' '}
                  {formatBytes((m.memory_mib || 0) * 1024 * 1024)} · {memPct}%
                </span>
              </div>
            </article>
          )
        })}
      </div>
    </div>
  )
}
