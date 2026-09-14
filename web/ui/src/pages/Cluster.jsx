import { Link, useLocation, useOutletContext } from 'react-router-dom'
import { asList, formatBytes, publicIpv6 } from '../api'
import { Icon } from '../components/Icons'
import { parseResourceRoute, resourceLink } from '../resourceRoutes'

function meterTone(pct) {
  if (pct >= 90) return 'hot'
  if (pct >= 75) return 'warm'
  return ''
}

function UsageBar({ label, pct, sublabel }) {
  const n = Number.isFinite(pct) ? Math.max(0, Math.min(100, pct)) : 0
  return (
    <div className="pve-usage-bar">
      <div className="pve-usage-bar-head">
        <span>{label}</span>
        <em>
          {n.toFixed(1)}%
          {sublabel ? <span className="muted">{sublabel}</span> : null}
        </em>
      </div>
      <div className="pve-usage-track">
        <div className={`pve-usage-fill ${meterTone(n)}`} style={{ width: `${Math.max(2, n)}%` }} />
      </div>
    </div>
  )
}

export default function Cluster() {
  const { inv } = useOutletContext()
  const { cluster, error, setError } = inv
  const members = asList(cluster?.members)
  const currentRoute = parseResourceRoute(useLocation().pathname)

  return (
    <div className="pve-tab-page">
      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}

      {members.length === 0 ? (
        <div className="pve-empty">
          <Icon name="cluster" size={22} />
          <span className="muted">No cluster members yet.</span>
        </div>
      ) : (
        <div className="cluster-grid">
          {members.map((m) => {
            const cpuPct = m.cpus ? (m.used_vcpus / m.cpus) * 100 : 0
            const memPct = m.memory_mib ? (m.used_memory_mib / m.memory_mib) * 100 : 0
            const v6 = publicIpv6(m.ipv6)
            const addr = m.ipv4?.[0] || v6[0] || m.peer_url || '—'
            const isSelf = m.id === cluster?.self_id
            const isLeader = m.id === cluster?.leader_id
            return (
              <Link
                key={m.id}
                to={resourceLink('node', m.id, currentRoute)}
                className={`cluster-card${m.online ? '' : ' offline'}`}
                title={m.name}
              >
                <div className="cluster-card-head">
                  <span className="cluster-card-title">
                    <span className={`cluster-card-dot ${m.online ? 'on' : 'off'}`} />
                    <strong className="cluster-card-name">{m.name || m.id}</strong>
                  </span>
                  <span className="cluster-card-ip">{addr}</span>
                </div>
                {(isSelf || isLeader) && (
                  <div className="cluster-card-badges">
                    {isSelf && <span className="pve-pill">this</span>}
                    {isLeader && <span className="pve-pill ok">leader</span>}
                  </div>
                )}
                <div className="cluster-card-meters">
                  <UsageBar
                    label="CPU"
                    pct={cpuPct}
                    sublabel={`${m.used_vcpus || 0}/${m.cpus || 0}`}
                  />
                  <UsageBar
                    label="Memory"
                    pct={memPct}
                    sublabel={formatBytes((m.memory_mib || 0) * 1024 * 1024)}
                  />
                </div>
              </Link>
            )
          })}
        </div>
      )}
    </div>
  )
}
