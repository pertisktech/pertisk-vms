import { Link, useOutletContext } from 'react-router-dom'
import { asList, disksOf, formatBytes, isTemplate } from '../api'
import MetricCard from '../components/MetricCard'
import MetricsCharts from '../components/MetricsCharts'
import { useMetrics } from '../useMetrics'

function pctLabel(n) {
  if (n == null || !Number.isFinite(n)) return '—'
  return `${n >= 10 ? n.toFixed(0) : n.toFixed(1)}%`
}

function stateClass(state) {
  if (state === 'running') return 'ready'
  if (state === 'failed') return 'error'
  if (state === 'created') return 'pending'
  return 'unknown'
}

export default function Overview() {
  const { inv } = useOutletContext()
  const { host, cluster, vms, error, loading } = inv
  const metrics = useMetrics('cluster')
  const members = asList(cluster?.members)
  const online = members.filter((m) => m.online).length
  const guests = vms.filter((vm) => !isTemplate(vm))
  const running = guests.filter((vm) => vm.state === 'running').length

  return (
    <div className="pve-stack">
      {error && error !== 'unauthorized' && <div className="banner danger">{error}</div>}

      <div className="dash-stat-row">
        <MetricCard
          label="CPU load"
          value={pctLabel(Number(metrics.data?.live?.cpu_pct))}
          hint={
            Number.isFinite(Number(metrics.data?.live?.cpu_pct))
              ? Number(metrics.data.live.cpu_pct) < 70
                ? 'Normal'
                : 'High'
              : undefined
          }
          hintTone={Number(metrics.data?.live?.cpu_pct) < 70 ? 'ok' : undefined}
          pct={Number(metrics.data?.live?.cpu_pct)}
        />
        <MetricCard
          label="Memory"
          value={pctLabel(
            Number(metrics.data?.live?.mem_total_bytes) > 0
              ? (Number(metrics.data.live.mem_used_bytes) / Number(metrics.data.live.mem_total_bytes)) * 100
              : null,
          )}
          hint={
            Number(metrics.data?.live?.mem_total_bytes) > 0
              ? `${formatBytes(metrics.data.live.mem_used_bytes)} / ${formatBytes(metrics.data.live.mem_total_bytes)}`
              : undefined
          }
          pct={
            Number(metrics.data?.live?.mem_total_bytes) > 0
              ? (Number(metrics.data.live.mem_used_bytes) / Number(metrics.data.live.mem_total_bytes)) * 100
              : null
          }
        />
        <MetricCard
          label="Virtual machines"
          value={`${running} / ${guests.length}`}
          hint={`${running} running`}
          hintTone="ok"
          pct={guests.length ? (running / guests.length) * 100 : 0}
          barTone="ok"
        />
        <MetricCard
          label="Nodes"
          value={`${online} / ${members.length || 1}`}
          hint={cluster?.quorum ? 'Quorum held' : 'No quorum'}
          hintTone={cluster?.quorum ? 'ok' : undefined}
          pct={(online / (members.length || 1)) * 100}
          barTone={cluster?.quorum ? 'ok' : 'hot'}
        />
      </div>

      <MetricsCharts
        scope="cluster"
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
        {loading && !guests.length ? (
          <p className="muted">Loading…</p>
        ) : guests.length === 0 ? (
          <div className="dash-empty card">
            <strong>No guests yet</strong>
            <p className="muted">Use Create guest in the header to start a machine, or clone a cloud template.</p>
          </div>
        ) : (
          <div className="guest-grid">
            {guests.slice(0, 8).map((vm) => (
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
