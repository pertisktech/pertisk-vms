import { useEffect, useMemo, useRef, useState } from 'react'
import { api, disksOf, formatBytes, netsOf } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

const ADD_KINDS = [
  { key: 'disk', label: 'Hard Disk', icon: 'disk' },
  { key: 'cdrom', label: 'CD/DVD Drive', icon: 'volumes' },
  { key: 'nic', label: 'Network Device', icon: 'network' },
]

export default function GuestHardware() {
  const { vm, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const [addOpen, setAddOpen] = useState(false)
  const [dialog, setDialog] = useState(null)
  const [form, setForm] = useState({})
  const addRef = useRef(null)

  const running = vm.state === 'running'
  const freeVolumes = useMemo(() => {
    const used = new Set(
      inv.vms.flatMap((item) => disksOf(item).map((d) => d.volume_id).filter(Boolean)),
    )
    return inv.volumes.filter((v) => !used.has(v.id))
  }, [inv.vms, inv.volumes])

  useEffect(() => {
    if (!addOpen) return
    function onPointerDown(e) {
      if (addRef.current && !addRef.current.contains(e.target)) setAddOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    return () => document.removeEventListener('pointerdown', onPointerDown)
  }, [addOpen])

  function openDialog(kind) {
    setAddOpen(false)
    if (kind === 'disk') setForm({ volume_id: freeVolumes[0]?.id || '' })
    if (kind === 'cdrom') setForm({ iso: inv.isos[0]?.name || '' })
    if (kind === 'nic') setForm({ network_id: inv.networks[0]?.id || '', ip: '' })
    if (kind === 'memory') setForm({ memory_mib: vm.spec?.memory_mib || 512 })
    if (kind === 'cpu') setForm({ vcpus: vm.spec?.vcpus || 1 })
    if (kind === 'name') setForm({ name: vm.spec?.name || '', ha: vm.spec?.ha !== false })
    setDialog(kind)
  }

  async function submit(e) {
    e.preventDefault()
    const kind = dialog
    setDialog(null)
    await inv.mutate(() => {
      if (kind === 'disk') {
        return api(`/v1/vms/${vm.id}/disks`, {
          method: 'POST',
          body: { volume_id: form.volume_id },
        })
      }
      if (kind === 'cdrom') {
        return api(`/v1/vms/${vm.id}/cdrom`, { method: 'POST', body: { iso: form.iso } })
      }
      if (kind === 'nic') {
        return api(`/v1/vms/${vm.id}/nics`, {
          method: 'POST',
          body: { network_id: form.network_id, ip: form.ip?.trim() || undefined },
        })
      }
      const body =
        kind === 'memory'
          ? { memory_mib: Number(form.memory_mib) }
          : kind === 'cpu'
            ? { vcpus: Number(form.vcpus) }
            : { name: form.name.trim(), ha: form.ha }
      return api(`/v1/vms/${vm.id}`, { method: 'PATCH', body })
    })
  }

  async function remove(row) {
    const ok = await confirm({
      title: `Remove ${row.label}`,
      message: `Detach ${row.value} from ${vm.spec?.name || vm.id}?`,
      confirmLabel: 'Detach',
    })
    if (!ok) return
    await inv.mutate(() => api(row.removeUrl, { method: 'DELETE' }))
  }

  const rows = []
  rows.push({
    key: 'name',
    icon: 'guests',
    label: 'Name',
    value: vm.spec?.name || '—',
    edit: 'name',
  })
  rows.push({
    key: 'memory',
    icon: 'memory',
    label: 'Memory',
    value: `${vm.spec?.memory_mib || 0} MiB`,
    edit: 'memory',
    lockedWhileRunning: true,
  })
  rows.push({
    key: 'cpu',
    icon: 'cpu',
    label: 'Processors',
    value: `${vm.spec?.vcpus || 1} vCPU`,
    edit: 'cpu',
    lockedWhileRunning: true,
  })
  rows.push({
    key: 'ha',
    icon: 'cluster',
    label: 'High Availability',
    value: vm.spec?.ha !== false ? 'restart on node loss' : 'off',
    edit: 'name',
  })

  disksOf(vm)
    .filter((d) => !d.cdrom)
    .forEach((d, i) => {
      const vol = inv.volumes.find((v) => v.id === d.volume_id)
      rows.push({
        key: `disk-${d.volume_id || i}`,
        icon: 'disk',
        label: `Hard Disk (virtio${i})`,
        value: `${vol?.name || d.path || 'disk'}${vol?.size_bytes ? `, ${formatBytes(vol.size_bytes)}` : ''}`,
        removeUrl: d.volume_id ? `/v1/vms/${vm.id}/disks/${d.volume_id}` : null,
        lockedWhileRunning: true,
      })
    })

  disksOf(vm)
    .filter((d) => d.cdrom || d.iso_name)
    .forEach((d, i) => {
      rows.push({
        key: `cd-${d.iso_name || i}`,
        icon: 'volumes',
        label: 'CD/DVD Drive',
        value: d.iso_name || 'ISO',
        removeUrl: d.iso_name
          ? `/v1/vms/${vm.id}/cdrom/${encodeURIComponent(d.iso_name)}`
          : null,
        lockedWhileRunning: true,
      })
    })

  netsOf(vm).forEach((n, i) => {
    const net = inv.networks.find((item) => item.id === n.network_id)
    rows.push({
      key: `nic-${n.tap || n.mac || i}`,
      icon: 'network',
      label: `Network Device (net${i})`,
      value: [net?.name || n.tap || 'nic', n.ip, n.mac].filter(Boolean).join(', '),
      removeUrl: n.tap ? `/v1/vms/${vm.id}/nics/${encodeURIComponent(n.tap)}` : null,
      lockedWhileRunning: true,
    })
  })

  const addable = ADD_KINDS.filter((k) => {
    if (k.key === 'disk') return freeVolumes.length > 0
    if (k.key === 'cdrom') return inv.isos.length > 0
    return inv.networks.length > 0
  })

  return (
    <div className="pve-hw">
      {canWrite && (
        <div className="pve-hw-bar">
          <div className="pve-menu" ref={addRef}>
            <button
              type="button"
              className="btn-icon"
              disabled={running || addable.length === 0}
              onClick={() => setAddOpen((v) => !v)}
            >
              <Icon name="plus" size={15} />
              <span>Add</span>
              <Icon name="chevron-down" size={13} />
            </button>
            {addOpen && (
              <div className="pve-menu-list">
                {addable.map((k) => (
                  <button key={k.key} type="button" onClick={() => openDialog(k.key)}>
                    <Icon name={k.icon} size={15} />
                    {k.label}
                  </button>
                ))}
              </div>
            )}
          </div>
          {running && (
            <span className="muted">Stop the guest to change hardware. Name and HA stay editable.</span>
          )}
        </div>
      )}

      <div className="table-shell">
        <table className="pve-hw-table">
          <tbody>
            {rows.map((row) => {
              const locked = running && row.lockedWhileRunning
              return (
                <tr key={row.key}>
                  <td className="pve-hw-label">
                    <span>
                      <Icon name={row.icon} size={15} />
                      {row.label}
                    </span>
                  </td>
                  <td className="pve-hw-value">{row.value}</td>
                  <td className="pve-hw-act">
                    {canWrite && row.edit && (
                      <Btn variant="secondary" disabled={locked} onClick={() => openDialog(row.edit)}>
                        Edit
                      </Btn>
                    )}
                    {canWrite && row.removeUrl && (
                      <Btn variant="secondary" disabled={locked} onClick={() => remove(row)}>
                        Remove
                      </Btn>
                    )}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>

      {dialog && (
        <Modal
          title={
            {
              disk: 'Add hard disk',
              cdrom: 'Add CD/DVD drive',
              nic: 'Add network device',
              memory: 'Edit memory',
              cpu: 'Edit processors',
              name: 'Edit name and HA',
            }[dialog]
          }
          onClose={() => setDialog(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setDialog(null)}>
                Cancel
              </button>
              <button type="submit" form="hw-form">
                {dialog === 'disk' || dialog === 'cdrom' || dialog === 'nic' ? 'Add' : 'Save'}
              </button>
            </>
          }
        >
          <form id="hw-form" onSubmit={submit}>
            {dialog === 'disk' && (
              <div className="field">
                <label htmlFor="hw-vol">Volume</label>
                <select
                  id="hw-vol"
                  value={form.volume_id}
                  onChange={(e) => setForm({ volume_id: e.target.value })}
                >
                  {freeVolumes.map((vol) => (
                    <option key={vol.id} value={vol.id}>
                      {vol.name} · {formatBytes(vol.size_bytes)}
                    </option>
                  ))}
                </select>
              </div>
            )}
            {dialog === 'cdrom' && (
              <div className="field">
                <label htmlFor="hw-iso">ISO image</label>
                <select id="hw-iso" value={form.iso} onChange={(e) => setForm({ iso: e.target.value })}>
                  {inv.isos.map((item) => (
                    <option key={item.name} value={item.name}>
                      {item.name}
                    </option>
                  ))}
                </select>
              </div>
            )}
            {dialog === 'nic' && (
              <>
                <div className="field">
                  <label htmlFor="hw-net">Network</label>
                  <select
                    id="hw-net"
                    value={form.network_id}
                    onChange={(e) => setForm({ ...form, network_id: e.target.value })}
                  >
                    {inv.networks.map((n) => (
                      <option key={n.id} value={n.id}>
                        {n.name} · {n.cidr}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="hw-ip">Static IP (optional)</label>
                  <input
                    id="hw-ip"
                    value={form.ip}
                    onChange={(e) => setForm({ ...form, ip: e.target.value })}
                    placeholder="leave empty for DHCP"
                  />
                </div>
              </>
            )}
            {dialog === 'memory' && (
              <div className="field">
                <label htmlFor="hw-mem">Memory (MiB)</label>
                <input
                  id="hw-mem"
                  type="number"
                  min="64"
                  required
                  value={form.memory_mib}
                  onChange={(e) => setForm({ memory_mib: e.target.value })}
                />
              </div>
            )}
            {dialog === 'cpu' && (
              <div className="field">
                <label htmlFor="hw-cpu">vCPU</label>
                <input
                  id="hw-cpu"
                  type="number"
                  min="1"
                  required
                  value={form.vcpus}
                  onChange={(e) => setForm({ vcpus: e.target.value })}
                />
              </div>
            )}
            {dialog === 'name' && (
              <>
                <div className="field">
                  <label htmlFor="hw-name">Name</label>
                  <input
                    id="hw-name"
                    required
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                  />
                </div>
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={form.ha}
                    onChange={(e) => setForm({ ...form, ha: e.target.checked })}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">Restart elsewhere if this node is lost</span>
                </label>
              </>
            )}
          </form>
        </Modal>
      )}
    </div>
  )
}
