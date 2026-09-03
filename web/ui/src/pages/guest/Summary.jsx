import { useEffect, useState } from 'react'
import { api, asList, disksOf, netsOf, shortId } from '../../api'
import MetricsPanel from '../../components/MetricsPanel'
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
      return n.ip ? `${name} (${n.ip})` : name
    })
    .join(', ')
}

export default function GuestSummary() {
  const { vm: invVm, vmId, inv } = useGuest()
  const [vm, setVm] = useState(invVm)
  const { data: metrics } = useMetrics(vmId)

  useEffect(() => {
    setVm(invVm)
  }, [invVm])

  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const fresh = await api(`/v1/vms/${vmId}`)
        if (!cancelled && fresh) setVm(fresh)
      } catch {
        /* keep inventory copy */
      }
    }
    load()
    const id = setInterval(load, 5000)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [vmId])

  if (!vm) return null

  const disks = disksOf(vm).filter((d) => !d.cdrom)
  const cdroms = disksOf(vm).filter((d) => d.cdrom || d.iso_name)
  const running = vm.state === 'running'

  return (
    <div className="pve-stack">
      {vm.last_error && <div className="banner danger">{vm.last_error}</div>}

      <div className="dash-stat-row">
        <div className="stat">
          <div className="label">Status</div>
          <div className="value">{vm.state}</div>
        </div>
        <div className="stat">
          <div className="label">vCPU</div>
          <div className="value">{vm.spec?.vcpus || 1}</div>
        </div>
        <div className="stat">
          <div className="label">Memory</div>
          <div className="value">{vm.spec?.memory_mib || 0} MiB</div>
        </div>
        <div className="stat">
          <div className="label">Node</div>
          <div className="value">{nodeName(inv.cluster, vm.node_id)}</div>
        </div>
      </div>

      <MetricsPanel
        live={metrics?.live}
        title="Live"
        empty={running ? 'Collecting metrics…' : 'Guest stopped'}
      />

      <section className="card">
        <div className="table-meta">Configuration</div>
        <dl className="pve-kv">
          <dt>Name</dt>
          <dd>{vm.spec?.name || '—'}</dd>
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
          <dt>Serial log</dt>
          <dd className="mono-inline">{vm.serial_log || '—'}</dd>
          <dt>PID</dt>
          <dd className="mono-inline">{vm.pid || '—'}</dd>
        </dl>
      </section>
    </div>
  )
}
