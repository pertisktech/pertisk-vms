import { useState } from 'react'
import { api } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useGuest } from '../GuestView'

export default function GuestOptions() {
  const { vm, canWrite, inv } = useGuest()
  const [dialog, setDialog] = useState(null)
  const [form, setForm] = useState({})

  function openDialog(kind) {
    if (kind === 'name') setForm({ name: vm.spec?.name || '' })
    if (kind === 'ha') setForm({ ha: vm.spec?.ha !== false })
    if (kind === 'autostart')
      setForm({
        autostart: Boolean(vm.spec?.autostart),
        autostart_delay: vm.spec?.autostart_delay || 0,
        autostart_order: vm.spec?.autostart_order || 0,
      })
    setDialog(kind)
  }

  async function submit(e) {
    e.preventDefault()
    const kind = dialog
    setDialog(null)
    const body =
      kind === 'name'
        ? { name: form.name.trim() }
        : kind === 'ha'
          ? { ha: Boolean(form.ha) }
          : {
              autostart: Boolean(form.autostart),
              autostart_delay: Number(form.autostart_delay) || 0,
              autostart_order: Number(form.autostart_order) || 0,
            }
    await inv.mutate(() => api(`/v1/vms/${vm.id}`, { method: 'PATCH', body }))
  }

  const rows = [
    {
      key: 'name',
      icon: 'guests',
      label: 'Name',
      value: vm.spec?.name || '—',
      edit: 'name',
    },
    {
      key: 'ha',
      icon: 'cluster',
      label: 'High Availability',
      value: vm.spec?.ha !== false ? 'restart on node loss' : 'off',
      edit: 'ha',
    },
    {
      key: 'autostart',
      icon: 'play',
      label: 'Start at boot',
      value: vm.spec?.autostart ? 'yes' : 'no',
      edit: 'autostart',
    },
    {
      key: 'autostart-order',
      icon: 'options',
      label: 'Start order',
      value: String(vm.spec?.autostart_order || 0),
      edit: 'autostart',
    },
    {
      key: 'autostart-delay',
      icon: 'clock',
      label: 'Startup delay',
      value: `${vm.spec?.autostart_delay || 0} s`,
      edit: 'autostart',
    },
  ]

  return (
    <div className="pve-hw">
      <div className="pve-hw-bar">
        <span className="muted">Guest options. Start at boot powers the VM on when this node starts.</span>
      </div>

      <div className="table-shell">
        <table className="pve-hw-table">
          <tbody>
            {rows.map((row) => (
              <tr
                key={row.key}
                className={canWrite ? 'pve-hw-row-edit' : undefined}
                onClick={canWrite ? () => openDialog(row.edit) : undefined}
              >
                <td className="pve-hw-label">
                  <span>
                    <Icon name={row.icon} size={15} />
                    {row.label}
                  </span>
                </td>
                <td className="pve-hw-value">{row.value}</td>
                <td className="pve-hw-act">
                  {canWrite && (
                    <Btn variant="secondary" onClick={() => openDialog(row.edit)}>
                      Edit
                    </Btn>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {dialog && (
        <Modal
          title={
            {
              name: 'Edit name',
              ha: 'Edit high availability',
              autostart: 'Edit start at boot',
            }[dialog]
          }
          onClose={() => setDialog(null)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setDialog(null)}>
                Cancel
              </button>
              <button type="submit" form="opt-form">
                Save
              </button>
            </>
          }
        >
          <form id="opt-form" onSubmit={submit}>
            {dialog === 'name' && (
              <div className="field">
                <label htmlFor="opt-name">Name</label>
                <input
                  id="opt-name"
                  required
                  autoFocus
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </div>
            )}
            {dialog === 'ha' && (
              <label className="chk">
                <input
                  type="checkbox"
                  checked={form.ha}
                  onChange={(e) => setForm({ ...form, ha: e.target.checked })}
                />
                <span className="chk-box" />
                <span className="chk-label">
                  Restart on another node if this one is lost
                  <small>High availability</small>
                </span>
              </label>
            )}
            {dialog === 'autostart' && (
              <div className="wizard-options">
                <label className="chk">
                  <input
                    type="checkbox"
                    checked={form.autostart}
                    onChange={(e) => setForm({ ...form, autostart: e.target.checked })}
                  />
                  <span className="chk-box" />
                  <span className="chk-label">
                    Start at boot
                    <small>Power on when this node starts</small>
                  </span>
                </label>
                <div className="form-grid" style={{ marginTop: '0.85rem' }}>
                  <div className="field">
                    <label htmlFor="opt-as-order">Start order</label>
                    <input
                      id="opt-as-order"
                      type="number"
                      min="0"
                      value={form.autostart_order}
                      onChange={(e) => setForm({ ...form, autostart_order: e.target.value })}
                    />
                    <p className="field-hint">Lower numbers start first</p>
                  </div>
                  <div className="field">
                    <label htmlFor="opt-as-delay">Startup delay (seconds)</label>
                    <input
                      id="opt-as-delay"
                      type="number"
                      min="0"
                      value={form.autostart_delay}
                      onChange={(e) => setForm({ ...form, autostart_delay: e.target.value })}
                    />
                    <p className="field-hint">Wait after the node is up</p>
                  </div>
                </div>
              </div>
            )}
          </form>
        </Modal>
      )}
    </div>
  )
}
