import { formatBytes } from '../../api'
import { useNode } from '../NodeView'

function Meter({ label, used, total, unit }) {
  const pct = total > 0 ? Math.min(100, Math.round((used / total) * 100)) : 0
  return (
    <div className="pve-meter">
      <div className="pve-meter-head">
        <span>{label}</span>
        <span className="muted">
          {pct}% {total > 0 ? `(${used}${unit} of ${total}${unit})` : ''}
        </span>
      </div>
      <div className="pve-meter-track">
        <div
          className={`pve-meter-fill${pct > 90 ? ' hot' : pct > 75 ? ' warm' : ''}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}

export default function NodeSummary() {
  const { node, guests, inv } = useNode()
  const host = inv.host
  const running = guests.filter((vm) => vm.state === 'running')
  const usedMem = running.reduce((sum, vm) => sum + (vm.spec?.memory_mib || 0), 0)
  const usedCpu = running.reduce((sum, vm) => sum + (vm.spec?.vcpus || 0), 0)
  const totalMem = node?.memory_mib || host?.memory_mib || 0
  const totalCpu = node?.cpus || host?.cpus || 0

  return (
    <div className="pve-stack">
      <section className="card">
        <div className="table-meta">Status</div>
        <div className="pve-meters">
          <Meter label="CPU (allocated vCPU)" used={usedCpu} total={totalCpu} unit="" />
          <Meter label="Memory (allocated)" used={usedMem} total={totalMem} unit=" MiB" />
        </div>
      </section>

      <section className="card">
        <div className="table-meta">Node</div>
        <dl className="pve-kv">
          <dt>Name</dt>
          <dd>{node?.name || '—'}</dd>
          <dt>Status</dt>
          <dd>{node?.online === false ? 'offline' : 'online'}</dd>
          <dt>Guests</dt>
          <dd>
            {running.length} running / {guests.length} total
          </dd>
          <dt>Platform</dt>
          <dd>{host ? `${host.os}/${host.arch}` : '—'}</dd>
          <dt>Driver</dt>
          <dd>{host?.driver || '—'}</dd>
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
