import { Link, useLocation, useOutletContext } from 'react-router-dom'
import { asList, publicIpv6 } from '../api'
import { Icon } from '../components/Icons'
import { parseResourceRoute, resourceLink } from '../resourceRoutes'
import { useMetrics } from '../useMetrics'

function meterTone(pct) {
  if (pct >= 90) return 'hot'
  if (pct >= 75) return 'warm'
  return ''
}

function UsageBar({ label, value, sublabel }) {
  const n = Number.isFinite(value) ? Math.max(0, Math.min(100, value)) : 0
  return (
    <div className="pve-usage-bar">
      <div className="pve-usage-bar-head">
        <span>{label}</span>
        <span className="pve-usage-bar-val">
          {n.toFixed(1)}%
          {sublabel ? <span className="muted">{sublabel}</span> : null}
        </span>
      </div>
      <div className="pve-usage-track">
        <div className={`pve-usage-fill ${meterTone(n)}`} style={{ width: `${Math.max(2, n)}%` }} />
      </div>
    </div>
  )
}

function nodeAddress(member) {
  const v6 = publicIpv6(member?.ipv6)
  return member?.ipv4?.[0] || v6[0] || member?.peer_url || '—'
}

export default function Cluster() {
  const { inv } = useOutletContext()
  const { cluster, host, error, setError } = inv
  const metrics = useMetrics('cluster')
  const members = asList(cluster?.members)
  const currentRoute = parseResourceRoute(useLocation().pathname)
  const liveById = new Map(
    asList(metrics.data?.nodes).map((n) => [String(n.node_id), n]),
  )

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
            const sample = liveById.get(String(m.id))
            const live = sample?.live
            const cores = Number(m.cpus) || 0
            const cpuPct = Number.isFinite(Number(live?.cpu_pct))
              ? Number(live.cpu_pct)
              : cores
                ? (Number(m.used_vcpus) / cores) * 100
                : 0
            const memTotal =
              Number(live?.mem_total_bytes) ||
              (Number(m.memory_mib) || 0) * 1024 * 1024 ||
              (String(m.id) === String(cluster?.self_id) ? Number(host?.memory_mib || 0) * 1024 * 1024 : 0)
            const memUsed =
              Number(live?.mem_used_bytes) ||
              (Number(m.used_memory_mib) || 0) * 1024 * 1024
            const memPct = memTotal > 0 ? (memUsed / memTotal) * 100 : 0
            const usedCores = cores ? Math.round((cpuPct / 100) * cores) : 0
            const memTotalGiB = memTotal > 0 ? Math.round(memTotal / (1024 ** 3)) : 0
            const isSelf = m.id === cluster?.self_id
            const isLeader = m.id === cluster?.leader_id
            const online = m.online !== false

            return (
              <article key={m.id} className={`cluster-card${online ? '' : ' offline'}`}>
                <div className="cluster-card-head">
                  <div className="cluster-card-title">
                    <span className={`cluster-card-dot ${online ? 'on' : 'off'}`} />
                    <Link
                      to={resourceLink('node', m.id, currentRoute)}
                      className="cluster-card-name"
                      title={m.name || String(m.id)}
                    >
                      {m.name || m.id}
                    </Link>
                  </div>
                  <span className="cluster-card-ip">{nodeAddress(m)}</span>
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
                    value={cpuPct}
                    sublabel={cores ? `${usedCores}/${cores}` : undefined}
                  />
                  <UsageBar
                    label="Memory"
                    value={memPct}
                    sublabel={memTotalGiB ? `${memTotalGiB} GiB` : undefined}
                  />
                </div>
              </article>
            )
          })}
        </div>
      )}
    </div>
  )
}
