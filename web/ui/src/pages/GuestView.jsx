import { useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { api, asList } from '../api'
import { Btn } from '../components/Icons'
import Modal from '../components/Modal'
import ResourceView from '../components/ResourceView'
import { useConfirm } from '../components/Confirm'

export function useGuest() {
  const { vmId } = useParams()
  const ctx = useOutletContext()
  const vm = ctx.inv.vms.find((item) => item.id === vmId) || null
  return { ...ctx, vmId, vm }
}

function stateClass(state) {
  if (state === 'running') return 'ready'
  if (state === 'failed') return 'error'
  if (state === 'created') return 'pending'
  return 'unknown'
}

export default function GuestView() {
  const { vm, vmId, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const nav = useNavigate()
  const [migrateOpen, setMigrateOpen] = useState(false)
  const [migrateTarget, setMigrateTarget] = useState('')

  const peers = asList(inv.cluster?.members).filter((m) => m.online && m.id !== vm?.node_id)
  const running = vm?.state === 'running'

  async function act(kind) {
    if (kind === 'rm') {
      const ok = await confirm({
        title: 'Destroy guest',
        message: `Remove ${vm?.spec?.name || vmId} and disks that are not used by other guests? This cannot be undone.`,
        confirmLabel: 'Destroy',
      })
      if (!ok) return
      await inv.mutate(() => api(`/v1/vms/${vmId}`, { method: 'DELETE' }))
      nav('/dc/summary')
      return
    }
    await inv.mutate(() => api(`/v1/vms/${vmId}/${kind}`, { method: 'POST' }))
  }

  if (!vm) {
    return (
      <div className="pve-panel">
        {inv.loading ? (
          <p className="muted" style={{ padding: '1rem' }}>
            Loading…
          </p>
        ) : (
          <div className="dash-empty card">
            <strong>Guest not found</strong>
            <p className="muted">It may have been destroyed or migrated to another node.</p>
          </div>
        )}
      </div>
    )
  }

  return (
    <>
      <ResourceView
        icon="guests"
        kind="Guest"
        name={vm.spec?.name || vmId}
        status={
          <>
            <span className={`badge ${stateClass(vm.state)}`}>{vm.state}</span>
            {vm.spec?.ha !== false && <span className="badge pending">HA</span>}
          </>
        }
        tabs={[
          { to: 'summary', label: 'Summary', icon: 'summary' },
          { to: 'console', label: 'Console', icon: 'terminal' },
          { to: 'hardware', label: 'Hardware', icon: 'hardware' },
        ]}
        actions={
          canWrite && (
            <>
              {!running && (
                <Btn icon="play" variant="secondary" onClick={() => act('start')}>
                  Start
                </Btn>
              )}
              {running && (
                <Btn icon="stop" variant="secondary" onClick={() => act('stop')}>
                  Stop
                </Btn>
              )}
              {running && peers.length > 0 && (
                <Btn
                  icon="migrate"
                  variant="secondary"
                  onClick={() => {
                    setMigrateTarget(peers[0]?.id || '')
                    setMigrateOpen(true)
                  }}
                >
                  Migrate
                </Btn>
              )}
              <Btn icon="trash" variant="danger" onClick={() => act('rm')}>
                Destroy
              </Btn>
            </>
          )
        }
      />

      {migrateOpen && (
        <Modal
          title={`Migrate ${vm.spec?.name || vmId}`}
          hint="Pick an online node. Empty target lets the scheduler choose."
          onClose={() => setMigrateOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setMigrateOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="migrate-guest">
                Migrate
              </button>
            </>
          }
        >
          <form
            id="migrate-guest"
            onSubmit={(e) => {
              e.preventDefault()
              const target = migrateTarget || undefined
              setMigrateOpen(false)
              inv.mutate(() =>
                api(`/v1/vms/${vmId}/migrate`, {
                  method: 'POST',
                  body: target ? { target } : {},
                }),
              )
            }}
          >
            <div className="field">
              <label htmlFor="migrate-target">Target node</label>
              <select
                id="migrate-target"
                value={migrateTarget}
                onChange={(e) => setMigrateTarget(e.target.value)}
              >
                <option value="">Scheduler pick</option>
                {peers.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
            </div>
          </form>
        </Modal>
      )}
    </>
  )
}
