import { formatBytes, publicIpv6 } from '../../api'
import MetricCard from '../../components/MetricCard'
import MetricsCharts from '../../components/MetricsCharts'
import { useMetrics } from '../../useMetrics'
import { useNode } from '../NodeView'

function formatAddrs(value) {
  if (Array.isArray(value) && value.length) return value.join(', ')
  if (typeof value === 'string' && value) return value
  return '—'
}

function formatIpv6(value) {
  return formatAddrs(publicIpv6(value))
}

function pickAddrs(nodeAddrs, hostAddrs, self) {
  if (Array.isArray(nodeAddrs) && nodeAddrs.length) return nodeAddrs
  if (self) return hostAddrs
  return null
}

export default function NodeSummary() {
  const { node, guests, inv, nodeId } = useNode()
  const metrics = useMetrics('node')
  const host = inv.host
  const self = !inv.cluster?.self_id || inv.cluster.self_id === nodeId
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
  const ipv4 = formatAddrs(pickAddrs(node?.ipv4, host?.ipv4, self))
  const ipv6 = formatIpv6(pickAddrs(node?.ipv6, host?.ipv6, self))

  return (
    <div className="pve-stack">
      <div className="dash-stat-row">
        <MetricCard
          icon="check"
          label="Status"
          value={node?.online === false ? 'offline' : 'online'}
          hint={node?.online === false ? 'Down' : 'Live'}
          hintTone={node?.online === false ? undefined : 'ok'}
        />
        <MetricCard
          icon="guests"
          label="Guests"
          value={`${metrics.data?.running_vms ?? running.length} / ${guests.length}`}
          hint={`${metrics.data?.running_vms ?? running.length} running`}
          hintTone="ok"
          pct={guests.length ? ((metrics.data?.running_vms ?? running.length) / guests.length) * 100 : 0}
          barTone="ok"
        />
        <MetricCard
          icon="cpu"
          label="Allocated vCPU"
          value={`${metrics.data?.allocated_vcpus ?? usedCpu} / ${totalCpu || '—'}`}
          pct={totalCpu ? ((metrics.data?.allocated_vcpus ?? usedCpu) / totalCpu) * 100 : null}
        />
        <MetricCard
          icon="memory"
          label="Allocated memory"
          value={`${metrics.data?.allocated_memory_mib ?? usedMem} MiB`}
          hint={allocMemTotal ? `/ ${allocMemTotal} MiB` : undefined}
          pct={allocMemTotal ? ((metrics.data?.allocated_memory_mib ?? usedMem) / allocMemTotal) * 100 : null}
        />
      </div>

      <div className="node-addrs">
        <span>
          <span className="muted">IPv4</span> <span className="mono-inline">{ipv4}</span>
        </span>
        <span>
          <span className="muted">IPv6</span> <span className="mono-inline">{ipv6}</span>
        </span>
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
          <dt>IPv4</dt>
          <dd className="mono-inline">{ipv4}</dd>
          <dt>IPv6</dt>
          <dd className="mono-inline">{ipv6}</dd>
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
