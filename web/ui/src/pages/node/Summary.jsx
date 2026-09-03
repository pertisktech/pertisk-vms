import { formatBytes } from '../../api'
import MetricsPanel, { AllocatedPanel } from '../../components/MetricsPanel'
import { useMetrics } from '../../useMetrics'
import { useNode } from '../NodeView'

export default function NodeSummary() {
  const { node, guests, inv } = useNode()
  const { data: metrics } = useMetrics('node')
  const host = inv.host
  const running = guests.filter((vm) => vm.state === 'running')
  const usedMem = running.reduce((sum, vm) => sum + (vm.spec?.memory_mib || 0), 0)
  const usedCpu = running.reduce((sum, vm) => sum + (vm.spec?.vcpus || 0), 0)
  const totalCpu = node?.cpus || host?.cpus || 0
  const allocMemTotal =
    node?.memory_mib ||
    host?.memory_mib ||
    (metrics?.live?.mem_total_bytes
      ? Math.round(metrics.live.mem_total_bytes / (1024 * 1024))
      : 0)

  return (
    <div className="pve-stack">
      <MetricsPanel live={metrics?.live} title="Live" />
      <AllocatedPanel
        vcpusUsed={metrics?.allocated_vcpus ?? usedCpu}
        vcpusTotal={totalCpu || 1}
        memUsedMib={metrics?.allocated_memory_mib ?? usedMem}
        memTotalMib={allocMemTotal || 1}
      />

      <section className="card">
        <div className="table-meta">Node</div>
        <dl className="pve-kv">
          <dt>Name</dt>
          <dd>{node?.name || metrics?.name || '—'}</dd>
          <dt>Status</dt>
          <dd>{node?.online === false ? 'offline' : 'online'}</dd>
          <dt>Guests</dt>
          <dd>
            {metrics?.running_vms ?? running.length} running / {guests.length} total
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
