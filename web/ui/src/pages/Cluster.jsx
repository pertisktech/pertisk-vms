import { Link, useLocation, useOutletContext } from 'react-router-dom'
import { asList, formatBytes, publicIpv6 } from '../api'
import { Icon } from '../components/Icons'
import { parseResourceRoute, resourceLink } from '../resourceRoutes'

function meterTone(pct) {
  if (pct >= 90) return 'hot'
  if (pct >= 75) return 'warm'
  return ''
}

export default function Cluster() {
  const { inv } = useOutletContext()
  const { cluster, error, setError } = inv
  const members = asList(cluster?.members)
  const currentRoute = parseResourceRoute(useLocation().pathname)

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
          const addr = m.ipv4?.[0] || v6[0] || m.peer_url || '—'
          return (
            <Link
              key={m.id}
              to={resourceLink('node', m.id, currentRoute)}
              className={`cluster-card${m.online ? '' : ' offline'}`}
              title={m.name}
            >
              <div className="cluster-card-head">
                <span className={`cluster-card-dot ${m.online ? 'on' : 'off'}`} />
                <strong className="cluster-card-name">{m.name || m.id}</strong>
              </div>
              <div className="cluster-card-meta">
                {m.id === cluster?.self_id && <span>this</span>}
                {m.id === cluster?.leader_id && <span>leader</span>}
                <span className="cluster-card-ip">{addr}</span>
              </div>
              <div className="cluster-card-meters">
                <div className="cluster-card-meter">
                  <div className="cluster-card-track">
                    <div
                      className={`cluster-card-fill cpu ${meterTone(cpuPct)}`}
                      style={{ width: `${cpuPct}%` }}
                    />
                  </div>
                  <em>
                    {m.used_vcpus}/{m.cpus}
                  </em>
                </div>
                <div className="cluster-card-meter">
                  <div className="cluster-card-track">
                    <div
                      className={`cluster-card-fill mem ${meterTone(memPct)}`}
                      style={{ width: `${memPct}%` }}
                    />
                  </div>
                  <em>
                    {formatBytes((m.used_memory_mib || 0) * 1024 * 1024)}
                  </em>
                </div>
              </div>
            </Link>
          )
        })}
      </div>
    </div>
  )
}
