import { useEffect, useRef, useState } from 'react'
import { useOutletContext, useSearchParams } from 'react-router-dom'
import { api, asList, disksOf, getToken, netsOf } from '../api'
import { Btn, Icon } from '../components/Icons'
import GuestWizard from '../components/GuestWizard'
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
  const { vms, volumes, isos, networks, cluster, error, setError, mutate, refresh } = useInventory()
  const confirm = useConfirm()
  const [openCreate, setOpenCreate] = useState(() => params.get('new') === '1')
  const [selected, setSelected] = useState(null)
  const [consoleText, setConsoleText] = useState('')
  const [iso, setIso] = useState('')
  const [volumeId, setVolumeId] = useState('')
  const wsRef = useRef(null)
  const preRef = useRef(null)
  const selectedVm = vms.find((vm) => vm.id === selected) || null

  useEffect(() => {
    if (!isos.length) return
    if (!iso) setIso(isos[0].name)
  }, [isos, iso])

  useEffect(() => {
    if (!volumes.length) return
    if (!volumeId) setVolumeId(volumes[0].id)
  }, [volumes, volumeId])

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
        message: `Remove ${vm.spec?.name || vm.id}? This cannot be undone.`,
        confirmLabel: 'Destroy',
      })
      if (!ok) return
    }
    await mutate(async () => {
      if (kind === 'rm') await api(`/v1/vms/${vm.id}`, { method: 'DELETE' })
      else if (kind === 'migrate') await api(`/v1/vms/${vm.id}/migrate`, { method: 'POST', body: {} })
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
                <Icon name="terminal" size={18} />
                {selectedVm.spec?.name} console
              </h2>
              <p className="muted">Click the serial pane and type. Enter, Tab, and Backspace are forwarded.</p>
            </div>
            <div className="row-actions">
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
                  <button type="submit">Attach ISO</button>
                </form>
              )}
              {canWrite && volumes.length > 0 && (
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
                    {volumes.map((vol) => (
                      <option key={vol.id} value={vol.id}>
                        {vol.name}
                      </option>
                    ))}
                  </select>
                  <button type="submit">Attach volume</button>
                </form>
              )}
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

      {openCreate && (
        <GuestWizard
          volumes={volumes}
          isos={isos}
          networks={networks}
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
