import { Link, useLocation, useOutletContext } from 'react-router-dom'
import { api, asList, haDurable, publicIpv6 } from '../api'
import { Btn, Icon } from '../components/Icons'
import { useConfirm } from '../components/Confirm'
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
  try {
    const host = member?.peer_url ? new URL(member.peer_url).hostname : ''
    if (host) return host
  } catch {
    /* fall through */
  }
  const v6 = publicIpv6(member?.ipv6)
  return member?.ipv4?.[0] || v6[0] || member?.peer_url || '—'
}

export default function Cluster() {
  const { canWrite, inv } = useOutletContext()
  const { cluster, host, error, setError } = inv
  const confirm = useConfirm()
  const metrics = useMetrics('cluster')
  const members = asList(cluster?.members)
  const currentRoute = parseResourceRoute(useLocation().pathname)
  const liveById = new Map(
    asList(metrics.data?.nodes).map((n) => [String(n.node_id), n]),
  )
  const durable = haDurable(host)
  const haArmed = cluster?.ha_armed !== false
  const peers = members.filter((m) => m.id !== cluster?.self_id && m.online !== false)

  async function setHaArmed(armed) {
    if (!armed) {
      const ok = await confirm({
        title: 'Disarm HA',
        message:
          'Failover and quorum fencing pause until you arm HA again. Use this for planned node maintenance.',
        confirmLabel: 'Disarm',
      })
      if (!ok) return
    }
    await inv.mutate(() => api('/v1/cluster/ha', { method: 'POST', body: { armed } }))
  }

  async function drainSelf() {
    const ok = await confirm({
      title: 'Drain this node',
      message:
        'Running guests restart on another node (not live migrate). Continue?',
      confirmLabel: 'Drain',
    })
    if (!ok) return
    await inv.mutate(() => api('/v1/cluster/drain', { method: 'POST', body: {} }))
  }

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
      {!durable && (
        <div className="banner">
          Local replica storage is for lab use. HA restart does not protect unsynced writes. Production HA
          needs <code>storage.backend = &quot;rbd&quot;</code>.
        </div>
      )}
      {!haArmed && (
        <div className="banner">
          Cluster HA is disarmed. Nodes will not fail over guests or fence on quorum loss.
        </div>
      )}
      {canWrite && (
        <div className="pve-action-row">
          {haArmed ? (
            <Btn icon="stop" variant="secondary" onClick={() => setHaArmed(false)}>
              Disarm HA
            </Btn>
          ) : (
            <Btn icon="play" onClick={() => setHaArmed(true)}>
              Arm HA
            </Btn>
          )}
          {peers.length > 0 && (
            <Btn icon="migrate" variant="secondary" onClick={drainSelf}>
              Drain this node
            </Btn>
          )}
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
