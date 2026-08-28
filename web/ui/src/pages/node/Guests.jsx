import { Link } from 'react-router-dom'
import { api, disksOf, netsOf } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useConfirm } from '../../components/Confirm'
import { useNode } from '../NodeView'

function stateClass(state) {
  if (state === 'running') return 'ready'
  if (state === 'failed') return 'error'
  if (state === 'created') return 'pending'
  return 'unknown'
}

export default function NodeGuests() {
  const { guests, canWrite, inv } = useNode()
  const confirm = useConfirm()

  async function act(kind, vm) {
    if (kind === 'rm') {
      const ok = await confirm({
        title: 'Destroy guest',
        message: `Remove ${vm.spec?.name || vm.id} and disks that are not used by other guests? This cannot be undone.`,
        confirmLabel: 'Destroy',
      })
      if (!ok) return
    }
    await inv.mutate(async () => {
      if (kind === 'rm') await api(`/v1/vms/${vm.id}`, { method: 'DELETE' })
      else await api(`/v1/vms/${vm.id}/${kind}`, { method: 'POST' })
    })
  }

  if (!guests.length) {
    return (
      <div className="dash-empty card">
        <strong>No guests on this node</strong>
        <p className="muted">Use Create guest in the header to add one.</p>
      </div>
    )
  }

  return (
    <section className="card table-card">
      <div className="table-shell">
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Status</th>
              <th>vCPU</th>
              <th>Memory</th>
              <th>Disks</th>
              <th>NICs</th>
              <th>HA</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {guests.map((vm) => (
              <tr key={vm.id}>
                <td>
                  <Link to={`/vm/${vm.id}/summary`} className="pve-link">
                    <span className={`guest-orb ${vm.state}`} />
                    {vm.spec?.name || vm.id}
                  </Link>
                </td>
                <td>
                  <span className={`badge ${stateClass(vm.state)}`}>{vm.state}</span>
                </td>
                <td>{vm.spec?.vcpus || 1}</td>
                <td>{vm.spec?.memory_mib || 0} MiB</td>
                <td>{disksOf(vm).length}</td>
                <td>{netsOf(vm).length}</td>
                <td>{vm.spec?.ha !== false ? 'yes' : 'no'}</td>
                <td className="row-actions">
                  {canWrite && vm.state !== 'running' && (
                    <Btn icon="play" variant="secondary" onClick={() => act('start', vm)}>
                      Start
                    </Btn>
                  )}
                  {canWrite && vm.state === 'running' && (
                    <Btn icon="stop" variant="secondary" onClick={() => act('stop', vm)}>
                      Stop
                    </Btn>
                  )}
                  <Link to={`/vm/${vm.id}/console`} className="btn-icon secondary">
                    <Icon name="terminal" size={16} />
                    <span>Console</span>
                  </Link>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
