import { asList, disksOf, isTemplate, netsOf, nicAddrs, shortId } from '../../api'
import MetricCard from '../../components/MetricCard'
import MetricsCharts from '../../components/MetricsCharts'
import { useMetrics } from '../../useMetrics'
import { useGuest } from '../GuestView'

function nodeName(cluster, id) {
  const members = asList(cluster?.members)
  return members.find((m) => m.id === id)?.name || (id ? shortId(id) : '—')
}

function networkLine(vm, networks) {
  const nets = netsOf(vm)
  if (!nets.length) return 'none'
  return nets
    .map((n) => {
      const net = networks.find((item) => item.id === n.network_id)
      const name = net?.name || n.tap || 'nic'
      const addrs = nicAddrs(n)
      return addrs.length ? `${name} (${addrs.join(', ')})` : name
    })
    .join(', ')
}

export default function GuestSummary() {
  const { vm, vmId, inv } = useGuest()
  const metrics = useMetrics(vmId)

  if (!vm) return null

  const disks = disksOf(vm).filter((d) => !d.cdrom)
  const cdroms = disksOf(vm).filter((d) => d.cdrom || d.iso_name)
  const running = vm.state === 'running'

  return (
    <div className="pve-stack">
      {vm.last_error && <div className="banner danger">{vm.last_error}</div>}

      <div className="dash-stat-row">
        <MetricCard icon="check" label="Status" value={vm.state} hint={running ? 'Live' : vm.state} hintTone={running ? 'ok' : undefined} />
        <MetricCard icon="cpu" label="vCPU" value={String(vm.spec?.vcpus || 1)} hint="cores" />
        <MetricCard icon="memory" label="Memory" value={`${vm.spec?.memory_mib || 0} MiB`} />
        <MetricCard icon="worker" label="Node" value={nodeName(inv.cluster, vm.node_id)} />
      </div>

      <MetricsCharts
        scope="vm"
        title={isTemplate(vm) ? 'Template' : 'Guest resources'}
        history={metrics.history}
        latest={metrics.data}
        live={metrics.live}
        setLive={metrics.setLive}
        loading={metrics.loading}
        onRefresh={() => metrics.refresh()}
        empty={isTemplate(vm) ? 'Templates do not run' : running ? undefined : 'Guest stopped'}
      />

      <section className="card">
        <div className="table-meta">Configuration</div>
        <dl className="pve-kv">
          <dt>Name</dt>
          <dd>{vm.spec?.name || '—'}</dd>
          <dt>Type</dt>
          <dd>{isTemplate(vm) ? 'cloud template' : 'guest'}</dd>
          <dt>ID</dt>
          <dd className="mono-inline">{vm.id}</dd>
          <dt>High availability</dt>
          <dd>{vm.spec?.ha !== false ? 'restart on node loss' : 'off'}</dd>
          <dt>Start at boot</dt>
          <dd>
            {vm.spec?.autostart
              ? `yes${vm.spec?.autostart_order ? `, order ${vm.spec.autostart_order}` : ''}${
                  vm.spec?.autostart_delay ? `, delay ${vm.spec.autostart_delay}s` : ''
                }`
              : 'no'}
          </dd>
          <dt>Disks</dt>
          <dd>
            {disks.length === 0
              ? 'none'
              : disks
                  .map((d) => inv.volumes.find((v) => v.id === d.volume_id)?.name || d.path)
                  .join(', ')}
          </dd>
          <dt>CD-ROM</dt>
          <dd>{cdroms.length === 0 ? 'none' : cdroms.map((d) => d.iso_name || 'ISO').join(', ')}</dd>
          <dt>Network</dt>
          <dd>{networkLine(vm, inv.networks)}</dd>
          <dt>IPv4</dt>
          <dd className="mono-inline">
            {netsOf(vm)
              .map((n) => n.ip)
              .filter(Boolean)
              .join(', ') || '—'}
          </dd>
          <dt>IPv6</dt>
          <dd className="mono-inline">
            {netsOf(vm)
              .flatMap((n) => (n.ipv6 ? [n.ipv6] : []))
              .join(', ') || '—'}
          </dd>
          <dt>Serial log</dt>
          <dd className="mono-inline">{vm.serial_log || '—'}</dd>
          <dt>PID</dt>
          <dd className="mono-inline">{vm.pid || '—'}</dd>
        </dl>
      </section>
    </div>
  )
}
