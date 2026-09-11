import { useRef, useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { api, asList, isTemplate, vmCaption } from '../api'
import { Btn } from '../components/Icons'
import Modal from '../components/Modal'
import ResourceView from '../components/ResourceView'
import { useConfirm } from '../components/Confirm'
import CloneWizard from '../components/CloneWizard'

export function useGuest() {
  const { vmId } = useParams()
  const ctx = useOutletContext()
  const live = ctx.inv.vms.find((item) => String(item.id) === vmId) || null
  // Keep last-known guest across inventory polls so Console websockets are not torn down.
  const cached = useRef(live)
  if (live) cached.current = live
  if (cached.current && String(cached.current.id) !== String(vmId)) {
    cached.current = live
  }
  const vm = live || cached.current
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
  const [cloneOpen, setCloneOpen] = useState(false)

  const peers = asList(inv.cluster?.members).filter((m) => m.online && m.id !== vm?.node_id)
  const running = vm?.state === 'running'
  const template = isTemplate(vm)

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
    if (inv.loading && inv.vms.length === 0) {
      return (
        <div className="pve-panel">
          <p className="muted" style={{ padding: '1rem' }}>Loading…</p>
        </div>
      )
    }
    return (
      <div className="pve-panel">
        <div className="dash-empty card">
          <strong>Guest not found</strong>
          <p className="muted">It may have been destroyed or migrated to another node.</p>
        </div>
      </div>
    )
  }

  return (
    <>
      <ResourceView
        icon={template ? 'template' : 'guests'}
        kind={template ? 'Template' : 'Guest'}
        name={vmCaption(vm).title}
        status={
          <>
            {template ? (
              <span className="badge template">template</span>
            ) : (
              <span className={`badge ${stateClass(vm.state)}`}>{vm.state}</span>
            )}
            {!template && vm.spec?.ha !== false && <span className="badge pending">HA</span>}
            {!template && vm.spec?.autostart && <span className="badge pending">boot</span>}
          </>
        }
        tabs={[
          { to: 'summary', label: 'Summary', icon: 'summary' },
          !template && { to: 'console', label: 'Console', icon: 'terminal' },
          { to: 'hardware', label: 'Hardware', icon: 'hardware' },
          { to: 'options', label: 'Options', icon: 'options' },
        ].filter(Boolean)}
        actions={
          canWrite && (
            <>
              {template && (
                <Btn icon="clone" variant="secondary" onClick={() => setCloneOpen(true)}>
                  Clone
                </Btn>
              )}
              {!template && !running && (
                <Btn icon="play" variant="secondary" onClick={() => act('start')}>
                  Start
                </Btn>
              )}
              {!template && running && (
                <>
                  <Btn icon="power" variant="secondary" onClick={() => act('shutdown')} title="ACPI shutdown">
                    Shutdown
                  </Btn>
                  <Btn icon="refresh" variant="secondary" onClick={() => act('restart')} title="Hard reset">
                    Restart
                  </Btn>
                  <Btn icon="stop" variant="secondary" onClick={() => act('stop')} title="Force power off">
                    Stop
                  </Btn>
                </>
              )}
              {!template && running && peers.length > 0 && (
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
      {cloneOpen && (
        <CloneWizard
          source={vm}
          vms={inv.vms}
          volumes={inv.volumes}
          networks={inv.networks}
          cluster={inv.cluster}
          host={inv.host}
          onClose={() => setCloneOpen(false)}
          onCreated={inv.refresh}
        />
      )}
    </>
  )
}
