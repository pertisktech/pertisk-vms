import { useEffect, useMemo, useState } from 'react'
import { api, disksOf, netsOf } from '../../api'
import { Btn } from '../../components/Icons'
import { useGuest } from '../GuestView'

export default function GuestHardware() {
  const { vm, canWrite, inv } = useGuest()
  const [iso, setIso] = useState('')
  const [volumeId, setVolumeId] = useState('')
  const [networkId, setNetworkId] = useState('')
  const [nicIp, setNicIp] = useState('')
  const [edit, setEdit] = useState({ name: '', vcpus: 1, memory_mib: 512, ha: true })

  const running = vm.state === 'running'
  const freeVolumes = useMemo(() => {
    const used = new Set(
      inv.vms.flatMap((item) => disksOf(item).map((d) => d.volume_id).filter(Boolean)),
    )
    return inv.volumes.filter((v) => !used.has(v.id))
  }, [inv.vms, inv.volumes])
  const disks = disksOf(vm).filter((d) => !d.cdrom)
  const cdroms = disksOf(vm).filter((d) => d.cdrom || d.iso_name)

  useEffect(() => {
    setEdit({
      name: vm.spec?.name || '',
      vcpus: vm.spec?.vcpus || 1,
      memory_mib: vm.spec?.memory_mib || 512,
      ha: vm.spec?.ha !== false,
    })
  }, [vm.id])

  useEffect(() => {
    if (inv.isos.length && !iso) setIso(inv.isos[0].name)
  }, [inv.isos, iso])

  useEffect(() => {
    if (freeVolumes.length && !freeVolumes.some((v) => v.id === volumeId)) {
      setVolumeId(freeVolumes[0].id)
    }
  }, [freeVolumes, volumeId])

  useEffect(() => {
    if (inv.networks.length && !networkId) setNetworkId(inv.networks[0].id)
  }, [inv.networks, networkId])

  return (
    <div className="pve-stack">
      {canWrite && (
        <section className="card">
          <div className="table-meta">General</div>
          <form
            className="machine-edit"
            onSubmit={(e) => {
              e.preventDefault()
              const body = { name: edit.name.trim(), ha: edit.ha }
              if (!running) {
                body.vcpus = Number(edit.vcpus)
                body.memory_mib = Number(edit.memory_mib)
              }
              inv.mutate(() => api(`/v1/vms/${vm.id}`, { method: 'PATCH', body }))
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
                  disabled={running}
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
                  disabled={running}
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
          {running && (
            <p className="muted">
              vCPU and memory are locked while the guest runs. HA can change at any time.
            </p>
          )}
        </section>
      )}

      <section className="card">
        <div className="table-meta">Hard disks</div>
        {disks.length === 0 && <p className="muted">No data disks.</p>}
        {disks.map((d) => {
          const vol = inv.volumes.find((v) => v.id === d.volume_id)
          return (
            <div key={d.volume_id || d.path} className="hw-row">
              <span>{vol?.name || d.path || 'disk'}</span>
              {canWrite && d.volume_id && (
                <Btn
                  variant="secondary"
                  disabled={running}
                  onClick={() =>
                    inv.mutate(() =>
                      api(`/v1/vms/${vm.id}/disks/${d.volume_id}`, { method: 'DELETE' }),
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
              inv.mutate(() =>
                api(`/v1/vms/${vm.id}/disks`, { method: 'POST', body: { volume_id: volumeId } }),
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
            <button type="submit" disabled={running}>
              Attach
            </button>
          </form>
        )}
      </section>

      <section className="card">
        <div className="table-meta">CD/DVD drive</div>
        {cdroms.length === 0 && <p className="muted">No ISO attached.</p>}
        {cdroms.map((d) => (
          <div key={d.iso_name || 'cd'} className="hw-row">
            <span>{d.iso_name || 'ISO'}</span>
            {canWrite && d.iso_name && (
              <Btn
                variant="secondary"
                disabled={running}
                onClick={() =>
                  inv.mutate(() =>
                    api(`/v1/vms/${vm.id}/cdrom/${encodeURIComponent(d.iso_name)}`, {
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
        {canWrite && inv.isos.length > 0 && (
          <form
            className="inline-attach"
            onSubmit={(e) => {
              e.preventDefault()
              inv.mutate(() => api(`/v1/vms/${vm.id}/cdrom`, { method: 'POST', body: { iso } }))
            }}
          >
            <select value={iso} onChange={(e) => setIso(e.target.value)}>
              {inv.isos.map((item) => (
                <option key={item.name} value={item.name}>
                  {item.name}
                </option>
              ))}
            </select>
            <button type="submit" disabled={running}>
              Attach
            </button>
          </form>
        )}
      </section>

      <section className="card">
        <div className="table-meta">Network devices</div>
        {netsOf(vm).length === 0 && <p className="muted">No NICs.</p>}
        {netsOf(vm).map((n, i) => {
          const net = inv.networks.find((item) => item.id === n.network_id)
          return (
            <div key={n.tap || n.mac || i} className="hw-row">
              <span>
                {net?.name || n.tap || n.mac || 'nic'}
                {n.ip ? ` · ${n.ip}` : ''}
                {n.mac ? <span className="muted"> · {n.mac}</span> : null}
              </span>
              {canWrite && n.tap && (
                <Btn
                  variant="secondary"
                  disabled={running}
                  onClick={() =>
                    inv.mutate(() =>
                      api(`/v1/vms/${vm.id}/nics/${encodeURIComponent(n.tap)}`, {
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
        {canWrite && inv.networks.length > 0 && (
          <form
            className="inline-attach"
            onSubmit={(e) => {
              e.preventDefault()
              const ip = nicIp.trim()
              inv
                .mutate(() =>
                  api(`/v1/vms/${vm.id}/nics`, {
                    method: 'POST',
                    body: { network_id: networkId, ip: ip || undefined },
                  }),
                )
                .then(() => setNicIp(''))
            }}
          >
            <select value={networkId} onChange={(e) => setNetworkId(e.target.value)}>
              {inv.networks.map((n) => (
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
            <button type="submit" disabled={running}>
              Attach
            </button>
          </form>
        )}
      </section>
    </div>
  )
}
