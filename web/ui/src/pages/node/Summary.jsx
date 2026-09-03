import { formatBytes } from '../../api'
import MetricsCharts from '../../components/MetricsCharts'
import { useMetrics } from '../../useMetrics'
import { useNode } from '../NodeView'

export default function NodeSummary() {
  const { node, guests, inv } = useNode()
  const metrics = useMetrics('node')
  const host = inv.host
  const running = guests.filter((vm) => vm.state === 'running')
  const usedMem = running.reduce((sum, vm) => sum + (vm.spec?.memory_mib || 0), 0)
  const usedCpu = running.reduce((sum, vm) => sum + (vm.spec?.vcpus || 0), 0)
  const totalCpu = node?.cpus || host?.cpus || 0
  const allocMemTotal =
    node?.memory_mib ||
    host?.memory_mib ||
    (metrics.data?.live?.mem_total_bytes
      ? Math.round(metrics.data.live.mem_total_bytes / (1024 * 1024))
      : 0)

  return (
    <div className="pve-stack">
      <div className="dash-stat-row">
        <div className="stat">
          <div className="label">Status</div>
          <div className="value">{node?.online === false ? 'offline' : 'online'}</div>
        </div>
        <div className="stat">
          <div className="label">Guests</div>
          <div className="value">
            {metrics.data?.running_vms ?? running.length}
            <span className="muted" style={{ fontSize: '0.85rem', marginLeft: '0.4rem' }}>
              / {guests.length}
            </span>
          </div>
        </div>
        <div className="stat">
          <div className="label">Allocated vCPU</div>
          <div className="value">
            {metrics.data?.allocated_vcpus ?? usedCpu}
            <span className="muted" style={{ fontSize: '0.85rem', marginLeft: '0.4rem' }}>
              / {totalCpu || '—'}
            </span>
          </div>
        </div>
        <div className="stat">
          <div className="label">Allocated memory</div>
          <div className="value">
            {metrics.data?.allocated_memory_mib ?? usedMem} MiB
            <span className="muted" style={{ fontSize: '0.85rem', marginLeft: '0.4rem' }}>
              / {allocMemTotal || '—'}
            </span>
          </div>
        </div>
      </div>

      <MetricsCharts
        scope="node"
        title="Node resources"
        history={metrics.history}
        latest={metrics.data}
        live={metrics.live}
        setLive={metrics.setLive}
        loading={metrics.loading}
        onRefresh={() => metrics.refresh()}
      />

      <section className="card">
        <div className="table-meta">Node</div>
        <dl className="pve-kv">
          <dt>Name</dt>
          <dd>{node?.name || metrics.data?.name || '—'}</dd>
          <dt>Status</dt>
          <dd>{node?.online === false ? 'offline' : 'online'}</dd>
          <dt>Guests</dt>
          <dd>
            {metrics.data?.running_vms ?? running.length} running / {guests.length} total
          </dd>
          <dt>Platform</dt>
          <dd>{host ? `${host.os}/${host.arch}` : '—'}</dd>
          <dt>Driver</dt>
          <dd>{host?.driver || '—'}</dd>
          <dt>Version</dt>
          <dd>{host?.version ? `v${host.version}` : '—'}</dd>
          <dt>KVM</dt>
          <dd>{host?.kvm ? 'available' : 'unavailable'}</dd>
          <dt>Firmware</dt>
          <dd className="mono-inline">{host?.firmware || 'not found (kernel boot only)'}</dd>
          <dt>Storage root</dt>
          <dd className="mono-inline">{host?.storage_root || '—'}</dd>
          <dt>Volumes</dt>
          <dd>
            {inv.volumes.length} volumes · {inv.isos.length} ISOs ·{' '}
            {formatBytes(inv.volumes.reduce((s, v) => s + (Number(v.size_bytes) || 0), 0))}
          </dd>
        </dl>
      </section>
    </div>
  )
}
