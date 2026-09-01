import { asList, disksOf, netsOf, shortId } from '../../api'
import { useGuest } from '../GuestView'

function nodeName(cluster, id) {
  const members = asList(cluster?.members)
  return members.find((m) => m.id === id)?.name || (id ? shortId(id) : '—')
}

export default function GuestSummary() {
  const { vm, inv } = useGuest()
  const disks = disksOf(vm).filter((d) => !d.cdrom)
  const cdroms = disksOf(vm).filter((d) => d.cdrom || d.iso_name)

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
          <dd>
            {netsOf(vm).length === 0
              ? 'none'
              : netsOf(vm)
                  .map((n) => {
                    const net = inv.networks.find((item) => item.id === n.network_id)
                    return `${net?.name || n.tap || 'nic'}${n.ip ? ` (${n.ip})` : ''}`
                  })
                  .join(', ')}
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
