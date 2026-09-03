import { Link } from 'react-router-dom'
import { asList, disksOf } from '../api'
import MetricsCharts from '../components/MetricsCharts'
import { useInventory } from '../useInventory'
import { useMetrics } from '../useMetrics'

function stateClass(state) {
  if (state === 'running') return 'ready'
  if (state === 'failed') return 'error'
  if (state === 'created') return 'pending'
  return 'unknown'
}

export default function Overview() {
  const { host, cluster, vms, volumes, error, loading } = useInventory()
  const metrics = useMetrics('cluster')
  const members = asList(cluster?.members)
  const online = members.filter((m) => m.online).length
  const running = vms.filter((vm) => vm.state === 'running').length

  return (
    <div className="pve-stack">
      {error && <div className="banner danger">{error}</div>}

      <div className="dash-stat-row">
        <div className="stat">
          <div className="label">Guests live</div>
          <div className="value">
            {running}
            <span className="muted" style={{ fontSize: '0.85rem', marginLeft: '0.4rem' }}>
              / {vms.length}
            </span>
          </div>
        </div>
        <div className="stat">
          <div className="label">Nodes</div>
          <div className="value">
            {online}
            <span className="muted" style={{ fontSize: '0.85rem', marginLeft: '0.4rem' }}>
              / {members.length || 1}
            </span>
          </div>
        </div>
        <div className="stat">
          <div className="label">Quorum</div>
          <div className="value">{cluster?.quorum ? 'held' : 'lost'}</div>
        </div>
        <div className="stat">
          <div className="label">Volumes</div>
          <div className="value">{volumes.length}</div>
        </div>
      </div>

      <MetricsCharts
        title="Cluster resources"
        history={metrics.history}
        latest={metrics.data}
        nodes={asList(metrics.data?.nodes)}
        live={metrics.live}
        setLive={metrics.setLive}
        loading={metrics.loading}
        onRefresh={() => metrics.refresh()}
      />

      <section className="dash-panel">
        <div className="dash-resources-head">
          <div>
            <h2 className="card-title">Guests</h2>
            <p className="dash-section-sub muted">
              {host
                ? `${host.os}/${host.arch} · ${host.driver} · kvm ${host.kvm ? 'yes' : 'no'}`
                : 'Loading host…'}
            </p>
          </div>
        </div>
        {loading && !vms.length ? (
          <p className="muted">Loading…</p>
        ) : vms.length === 0 ? (
          <div className="dash-empty card">
            <strong>No guests yet</strong>
            <p className="muted">Use Create guest in the header to start a machine.</p>
          </div>
        ) : (
          <div className="guest-grid">
            {vms.slice(0, 8).map((vm) => (
              <Link key={vm.id} to={`/vm/${vm.id}/summary`} className="guest-card">
                <div className="guest-card-top">
                  <span className={`guest-orb ${vm.state}`} />
                  <strong>{vm.spec?.name || vm.id}</strong>
                  <span className={`badge ${stateClass(vm.state)}`}>{vm.state}</span>
                </div>
                <div className="guest-meta">
                  <span>
                    {vm.spec?.vcpus || 1} vCPU · {vm.spec?.memory_mib || 0} MiB
                  </span>
                  <span>
                    {disksOf(vm).length} disk{disksOf(vm).length === 1 ? '' : 's'}
                  </span>
                </div>
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
