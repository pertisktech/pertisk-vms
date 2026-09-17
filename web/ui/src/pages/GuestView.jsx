import { createContext, useContext, useRef, useState } from 'react'
import { Navigate, useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { api, asList, isTemplate, vmCaption } from '../api'
import { Btn } from '../components/Icons'
import Modal from '../components/Modal'
import ResourceView from '../components/ResourceView'
import { useConfirm } from '../components/Confirm'
import CloneWizard from '../components/CloneWizard'

const GuestOverride = createContext(null)

/** Optional override for standalone SSH popup windows. */
export function GuestProvider({ value, children }) {
  return <GuestOverride.Provider value={value}>{children}</GuestOverride.Provider>
}

export function useGuest() {
  const override = useContext(GuestOverride)
  const { vmId: routeId } = useParams()
  const ctx = useOutletContext() || {}
  const vmId = override?.vmId || routeId
  const live =
    override?.vm ||
    ctx.inv?.vms?.find((item) => String(item.id) === String(vmId)) ||
    null
  // Keep last-known guest across inventory polls so Console/SSH websockets are not torn down.
  // Clear when inventory confirms the guest is gone (terraform destroy / peer delete).
  const cached = useRef(live)
  const loading = Boolean(ctx.inv?.loading)
  if (live) {
    cached.current = live
  } else if (cached.current && String(cached.current.id) !== String(vmId)) {
    cached.current = null
  } else if (!loading && !override?.vm) {
    cached.current = null
  }
  const vm = live || cached.current
  if (override) {
    return { ...ctx, ...override, vmId, vm }
  }
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
  const nodeName =
    asList(inv.cluster?.members).find((m) => m.id === vm?.node_id)?.name ||
    inv.host?.hostname ||
    'node'
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
    return <Navigate to="/dc/summary" replace />
  }

  return (
    <>
      <ResourceView
        icon={template ? 'template' : 'guests'}
        kind={template ? 'Template' : 'Guest'}
        name={vmCaption(vm).title}
        crumbs={['Datacenter', nodeName, vm.spec?.name || vmCaption(vm).title]}
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
          { to: 'summary', label: 'Summary', icon: 'gauge' },
          !template && { to: 'console', label: 'Console', icon: 'terminal' },
          !template && { to: 'ssh', label: 'SSH', icon: 'key' },
          { to: 'hardware', label: 'Hardware', icon: 'hardware' },
          !template && { to: 'snapshots', label: 'Snapshots', icon: 'camera' },
          !template && { to: 'backup', label: 'Backup', icon: 'archive' },
          { to: 'tasks', label: 'Task History', icon: 'activity' },
          { to: 'options', label: 'Options', icon: 'options' },
        ].filter(Boolean)}
        actions={
          <>
            {!template && (
              <>
                <Btn icon="terminal" onClick={() => nav(`/vm/${vmId}/console`)}>
                  Console
                </Btn>
                <Btn icon="key" variant="secondary" onClick={() => nav(`/vm/${vmId}/ssh`)}>
                  SSH
                </Btn>
              </>
            )}
            {canWrite && (
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
                    <Btn icon="stop" variant="secondary" onClick={() => act('stop')} title="Force power off">
                      Stop
                    </Btn>
                    <Btn icon="refresh" variant="secondary" onClick={() => act('restart')} title="Hard reset">
                      Reboot
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
            )}
          </>
        }
      />

      {migrateOpen && (
        <Modal
          title={`Migrate ${vm.spec?.name || vmId}`}
          hint="Restarts the guest on the target (not live migrate). Empty target lets the scheduler choose."
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
          onCreated={async (vmId) => {
            await inv.refresh()
            setCloneOpen(false)
            if (vmId != null && vmId !== '') nav(`/vm/${vmId}/console`)
          }}
        />
      )}
    </>
  )
}
