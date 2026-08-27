import { useEffect, useRef, useState } from 'react'
import { useOutletContext, useSearchParams } from 'react-router-dom'
import { api, asList, disksOf, getToken, netsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import GuestWizard from '../components/GuestWizard'
import Modal from '../components/Modal'
import { useConfirm } from '../components/Confirm'
import { useInventory } from '../useInventory'

function stateClass(state) {
  if (state === 'running') return 'ready'
  if (state === 'failed') return 'error'
  if (state === 'created') return 'pending'
  return 'unknown'
}

function nodeName(cluster, id) {
  const members = cluster?.members || []
  return members.find((m) => m.id === id)?.name || (id ? String(id).slice(0, 8) : '—')
}

export default function Guests() {
  const { canWrite } = useOutletContext()
  const [params, setParams] = useSearchParams()
  const { vms, volumes, isos, networks, cluster, host, error, setError, mutate, refresh } = useInventory()
  const confirm = useConfirm()
  const [openCreate, setOpenCreate] = useState(() => params.get('new') === '1')
  const [selected, setSelected] = useState(null)
  const [consoleText, setConsoleText] = useState('')
  const [iso, setIso] = useState('')
  const [volumeId, setVolumeId] = useState('')
  const [networkId, setNetworkId] = useState('')
  const [nicIp, setNicIp] = useState('')
  const [migrateVm, setMigrateVm] = useState(null)
  const [migrateTarget, setMigrateTarget] = useState('')
  const [edit, setEdit] = useState({ name: '', vcpus: 1, memory_mib: 512, ha: true })
  const wsRef = useRef(null)
  const preRef = useRef(null)
  const selectedVm = vms.find((vm) => vm.id === selected) || null
  const usedVolIds = new Set(
    vms.flatMap((vm) => disksOf(vm).map((d) => d.volume_id).filter(Boolean)),
  )
  const freeVolumes = volumes.filter((v) => !usedVolIds.has(v.id))
  const hasCdrom = disksOf(selectedVm).some((d) => d.cdrom || d.iso_name)
  const migratePeers = asList(cluster?.members).filter(
    (m) => m.online && m.id !== migrateVm?.node_id,
  )

  useEffect(() => {
    if (!isos.length) return
    if (!iso) setIso(isos[0].name)
  }, [isos, iso])

  useEffect(() => {
    const used = new Set(vms.flatMap((vm) => disksOf(vm).map((d) => d.volume_id).filter(Boolean)))
    const free = volumes.filter((v) => !used.has(v.id))
    if (!free.length) return
    if (!volumeId || used.has(volumeId)) setVolumeId(free[0].id)
  }, [volumes, vms, volumeId])

  useEffect(() => {
    if (!networks.length) return
    if (!networkId) setNetworkId(networks[0].id)
  }, [networks, networkId])

  useEffect(() => {
    if (!selectedVm) return
    setEdit({
      name: selectedVm.spec?.name || '',
      vcpus: selectedVm.spec?.vcpus || 1,
      memory_mib: selectedVm.spec?.memory_mib || 512,
      ha: selectedVm.spec?.ha !== false,
    })
  }, [selectedVm?.id])

  useEffect(() => {
    if (preRef.current) preRef.current.scrollTop = preRef.current.scrollHeight
  }, [consoleText])

  useEffect(() => {
    const ws = wsRef.current
    return () => {
      if (ws) {
        ws.onclose = null
        ws.close()
      }
    }
  }, [])

  function openConsole(vm) {
    setSelected(vm.id)
    setConsoleText('')
    if (wsRef.current) {
      wsRef.current.onclose = null
      wsRef.current.close()
      wsRef.current = null
    }
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(
      `${proto}//${location.host}/v1/vms/${vm.id}/console/ws?token=${encodeURIComponent(getToken())}`,
    )
    socket.onmessage = (e) => {
      setConsoleText((t) => t + e.data)
    }
    wsRef.current = socket
  }

  function sendKey(e) {
    const ws = wsRef.current
    if (!ws || ws.readyState !== 1) return
    if (e.key === 'Enter') {
      ws.send('\n')
      e.preventDefault()
    } else if (e.key === 'Backspace') {
      ws.send('\x7f')
      e.preventDefault()
    } else if (e.key === 'Tab') {
      ws.send('\t')
      e.preventDefault()
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      ws.send(e.key)
      e.preventDefault()
    }
  }

  async function act(kind, vm) {
    if (kind === 'rm') {
      const ok = await confirm({
        title: 'Destroy guest',
        message: `Remove ${vm.spec?.name || vm.id} and disks that are not used by other guests? This cannot be undone.`,
        confirmLabel: 'Destroy',
      })
      if (!ok) return
    }
    if (kind === 'migrate') {
      const peers = asList(cluster?.members).filter(
        (m) => m.online && m.id !== vm.node_id,
      )
      setMigrateVm(vm)
      setMigrateTarget(peers[0]?.id || '')
      return
    }
    await mutate(async () => {
      if (kind === 'rm') await api(`/v1/vms/${vm.id}`, { method: 'DELETE' })
      else await api(`/v1/vms/${vm.id}/${kind}`, { method: 'POST' })
    })
    if (kind === 'rm' && selected === vm.id) setSelected(null)
  }

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="guests" size={20} />
            Guests
          </h1>
          <p className="dash-lead muted">{vms.length} machine{vms.length === 1 ? '' : 's'} on the cluster.</p>
        </div>
        {canWrite && (
          <Btn icon="plus" onClick={() => setOpenCreate(true)}>
            Create guest
          </Btn>
        )}
      </div>
      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}

      {vms.length === 0 ? (
        <div className="dash-empty card">
          <strong>The stage is empty</strong>
          <p className="muted">Guests are machines. Create one, start it, then open its console.</p>
        </div>
      ) : (
        <div className="guest-grid">
          {vms.map((vm) => (
            <article
              key={vm.id}
              className={`guest-card${selected === vm.id ? ' selected' : ''}`}
              onClick={() => setSelected(vm.id)}
            >
              <div className="guest-card-top">
                <span className={`guest-orb ${vm.state}`} />
                <strong>{vm.spec?.name || vm.id}</strong>
                <span className={`badge ${stateClass(vm.state)}`}>{vm.state}</span>
                {vm.spec?.ha !== false && <span className="badge pending">HA</span>}
              </div>
              <div className="guest-meta">
                <span>
                  {vm.spec?.vcpus || 1} vCPU · {vm.spec?.memory_mib || 0} MiB
                </span>
                <span>{nodeName(cluster, vm.node_id)}</span>
                <span>
                  {disksOf(vm).length} disk{disksOf(vm).length === 1 ? '' : 's'} · {netsOf(vm).length} nic
                  {netsOf(vm).length === 1 ? '' : 's'}
                </span>
              </div>
              {vm.last_error && <p className="guest-err">{vm.last_error}</p>}
              <div className="row-actions" onClick={(e) => e.stopPropagation()}>
                <Btn icon="terminal" variant="secondary" onClick={() => openConsole(vm)}>
                  Console
                </Btn>
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
                {canWrite && vm.state === 'running' && asList(cluster?.members).length > 1 && (
                  <Btn icon="migrate" variant="secondary" onClick={() => act('migrate', vm)}>
                    Migrate
                  </Btn>
                )}
                {canWrite && (
                  <Btn icon="trash" variant="danger" onClick={() => act('rm', vm)}>
                    Destroy
                  </Btn>
                )}
              </div>
            </article>
          ))}
        </div>
      )}

      {selectedVm && (
        <section className="console-dock card">
          <div className="dash-resources-head">
            <div>
              <h2 className="card-title">
                <Icon name="guests" size={18} />
                {selectedVm.spec?.name} hardware
              </h2>
              <p className="muted">
                {selectedVm.state === 'running'
                  ? 'Stop the guest before changing disks, NICs, vCPU, or memory. HA can change while running.'
                  : 'Attach or detach disks, ISO, and networks while stopped.'}
              </p>
            </div>
          </div>
          {canWrite && (
            <form
              className="machine-edit"
              onSubmit={(e) => {
                e.preventDefault()
                const running = selectedVm.state === 'running'
                const body = {
                  name: edit.name.trim(),
                  ha: edit.ha,
                }
                if (!running) {
                  body.vcpus = Number(edit.vcpus)
                  body.memory_mib = Number(edit.memory_mib)
                }
                mutate(() =>
                  api(`/v1/vms/${selectedVm.id}`, { method: 'PATCH', body }),
                )
              }}
            >
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="edit-name">Name</label>
                  <input
                    id="edit-name"
                    required
                    value={edit.name}
                    onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label htmlFor="edit-cpu">vCPU</label>
                  <input
                    id="edit-cpu"
                    type="number"
                    min="1"
                    disabled={selectedVm.state === 'running'}
                    value={edit.vcpus}
                    onChange={(e) => setEdit({ ...edit, vcpus: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label htmlFor="edit-mem">Memory MiB</label>
                  <input
                    id="edit-mem"
                    type="number"
                    min="64"
                    disabled={selectedVm.state === 'running'}
                    value={edit.memory_mib}
                    onChange={(e) => setEdit({ ...edit, memory_mib: e.target.value })}
                  />
                </div>
              </div>
              <div className="machine-edit-row">
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={edit.ha}
                    onChange={(e) => setEdit({ ...edit, ha: e.target.checked })}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">Restart elsewhere if this node is lost</span>
                </label>
                <button type="submit">Save</button>
              </div>
            </form>
          )}
          <div className="hw-grid">
            <div>
              <p className="wizard-section-title">Disks</p>
              {disksOf(selectedVm).filter((d) => !d.cdrom).length === 0 && (
                <p className="muted">No data disks.</p>
              )}
              {disksOf(selectedVm)
                .filter((d) => !d.cdrom)
                .map((d) => {
                  const vol = volumes.find((v) => v.id === d.volume_id)
                  return (
                    <div key={d.volume_id || d.path} className="hw-row">
                      <span>{vol?.name || d.path || 'disk'}</span>
                      {canWrite && d.volume_id && (
                        <Btn
                          variant="secondary"
                          disabled={selectedVm.state === 'running'}
                          onClick={() =>
                            mutate(() =>
                              api(`/v1/vms/${selectedVm.id}/disks/${d.volume_id}`, {
                                method: 'DELETE',
                              }),
                            )
                          }
                        >
                          Detach
                        </Btn>
                      )}
                    </div>
                  )
                })}
              {canWrite && freeVolumes.length > 0 && (
                <form
                  className="inline-attach"
                  onSubmit={(e) => {
                    e.preventDefault()
                    mutate(() =>
                      api(`/v1/vms/${selectedVm.id}/disks`, {
                        method: 'POST',
                        body: { volume_id: volumeId },
                      }),
                    )
                  }}
                >
                  <select value={volumeId} onChange={(e) => setVolumeId(e.target.value)}>
                    {freeVolumes.map((vol) => (
                      <option key={vol.id} value={vol.id}>
                        {vol.name}
                      </option>
                    ))}
                  </select>
                  <button type="submit" disabled={selectedVm.state === 'running'}>
                    Attach
                  </button>
                </form>
              )}
            </div>
            <div>
              <p className="wizard-section-title">CD-ROM</p>
              {!hasCdrom && <p className="muted">No ISO attached.</p>}
              {disksOf(selectedVm)
                .filter((d) => d.cdrom || d.iso_name)
                .map((d) => (
                  <div key={d.iso_name || 'cd'} className="hw-row">
                    <span>{d.iso_name || 'ISO'}</span>
                    {canWrite && d.iso_name && (
                      <Btn
                        variant="secondary"
                        disabled={selectedVm.state === 'running'}
                        onClick={() =>
                          mutate(() =>
                            api(`/v1/vms/${selectedVm.id}/cdrom/${encodeURIComponent(d.iso_name)}`, {
                              method: 'DELETE',
                            }),
                          )
                        }
                      >
                        Detach
                      </Btn>
                    )}
                  </div>
                ))}
              {canWrite && isos.length > 0 && (
                <form
                  className="inline-attach"
                  onSubmit={(e) => {
                    e.preventDefault()
                    mutate(() =>
                      api(`/v1/vms/${selectedVm.id}/cdrom`, { method: 'POST', body: { iso } }),
                    )
                  }}
                >
                  <select value={iso} onChange={(e) => setIso(e.target.value)}>
                    {isos.map((item) => (
                      <option key={item.name} value={item.name}>
                        {item.name}
                      </option>
                    ))}
                  </select>
                  <button type="submit" disabled={selectedVm.state === 'running'}>
                    Attach
                  </button>
                </form>
              )}
            </div>
            <div>
              <p className="wizard-section-title">NICs</p>
              {netsOf(selectedVm).length === 0 && <p className="muted">No NICs.</p>}
              {netsOf(selectedVm).map((n, i) => {
                const net = networks.find((item) => item.id === n.network_id)
                return (
                <div key={n.tap || n.mac || i} className="hw-row">
                  <span>
                    {net?.name || n.tap || n.mac || 'nic'}
                    {n.ip ? ` · ${n.ip}` : ''}
                  </span>
                  {canWrite && n.tap && (
                    <Btn
                      variant="secondary"
                      disabled={selectedVm.state === 'running'}
                      onClick={() =>
                        mutate(() =>
                          api(`/v1/vms/${selectedVm.id}/nics/${encodeURIComponent(n.tap)}`, {
                            method: 'DELETE',
                          }),
                        )
                      }
                    >
                      Detach
                    </Btn>
                  )}
                </div>
                )
              })}
              {canWrite && networks.length > 0 && (
                <form
                  className="inline-attach"
                  onSubmit={(e) => {
                    e.preventDefault()
                    const ip = nicIp.trim()
                    mutate(() =>
                      api(`/v1/vms/${selectedVm.id}/nics`, {
                        method: 'POST',
                        body: { network_id: networkId, ip: ip || undefined },
                      }),
                    ).then(() => setNicIp(''))
                  }}
                >
                  <select value={networkId} onChange={(e) => setNetworkId(e.target.value)}>
                    {networks.map((n) => (
                      <option key={n.id} value={n.id}>
                        {n.name}
                      </option>
                    ))}
                  </select>
                  <input
                    value={nicIp}
                    onChange={(e) => setNicIp(e.target.value)}
                    placeholder="IP (optional)"
                    aria-label="Static IP"
                  />
                  <button type="submit" disabled={selectedVm.state === 'running'}>
                    Attach
                  </button>
                </form>
              )}
            </div>
          </div>
        </section>
      )}

      {selectedVm && (
        <section className="console-dock card">
          <div className="dash-resources-head">
            <div>
              <h2 className="card-title">
                <Icon name="terminal" size={18} />
                {selectedVm.spec?.name} console
              </h2>
              <p className="muted">Click the serial pane and type. Enter, Tab, and Backspace are forwarded.</p>
            </div>
          </div>
          <pre
            ref={preRef}
            className="console-pane"
            tabIndex={0}
            onKeyDown={sendKey}
            onClick={() => preRef.current?.focus()}
          >
            {consoleText || 'Select Console on a guest, then type here.'}
          </pre>
        </section>
      )}

      {migrateVm && (
        <Modal
          title={`Migrate ${migrateVm.spec?.name || migrateVm.id}`}
          hint="Pick an online node. Empty target lets the scheduler choose."
          onClose={() => setMigrateVm(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setMigrateVm(null)}>
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
              const id = migrateVm.id
              const target = migrateTarget || undefined
              setMigrateVm(null)
              mutate(() =>
                api(`/v1/vms/${id}/migrate`, {
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
                {migratePeers.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                    {m.id === cluster?.self_id ? ' (this node)' : ''}
                  </option>
                ))}
              </select>
            </div>
          </form>
        </Modal>
      )}

      {openCreate && (
        <GuestWizard
          volumes={volumes}
          isos={isos}
          networks={networks}
          host={host}
          onClose={() => {
            setOpenCreate(false)
            if (params.get('new')) {
              const next = new URLSearchParams(params)
              next.delete('new')
              setParams(next, { replace: true })
            }
          }}
          onCreated={async () => {
            setOpenCreate(false)
            if (params.get('new')) {
              const next = new URLSearchParams(params)
              next.delete('new')
              setParams(next, { replace: true })
            }
            await refresh()
          }}
        />
      )}
    </div>
  )
}
