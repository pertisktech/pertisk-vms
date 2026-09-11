import { useState } from 'react'
import { api, isTemplate } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

export default function GuestOptions() {
  const { vm, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const [dialog, setDialog] = useState(null)
  const [form, setForm] = useState({})
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const template = isTemplate(vm)
  const running = vm?.state === 'running'

  function openDialog(kind) {
    setError('')
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
    if (kind === 'name' && !body.name) {
      setError('Name is required.')
      return
    }
    setBusy(true)
    setError('')
    try {
      await inv.mutate(() => api(`/v1/vms/${vm.id}`, { method: 'PATCH', body }))
      setDialog(null)
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
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
        <span className="muted">
          {template
            ? 'Template options. Clone this image to create guests; it cannot be started.'
            : 'Guest options. Start at boot powers the VM on when this node starts.'}
        </span>
        {canWrite && !template && !running && (
          <Btn
            icon="template"
            variant="secondary"
            onClick={async () => {
              const ok = await confirm({
                title: 'Convert to template',
                message: `Turn ${vm.spec?.name || vm.id} into a cloud template? It cannot be started afterward; clone it to create guests.`,
                confirmLabel: 'Convert',
                tone: 'primary',
              })
              if (!ok) return
              await inv.mutate(() => api(`/v1/vms/${vm.id}/template`, { method: 'POST' }))
            }}
          >
            Convert to template
          </Btn>
        )}
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
              <button type="button" className="secondary" onClick={() => setDialog(null)} disabled={busy}>
                Cancel
              </button>
              <button type="submit" form="opt-form" disabled={busy}>
                {busy ? 'Saving…' : 'Save'}
              </button>
            </>
          }
        >
          {error && <div className="error">{error}</div>}
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
                <p className="field-hint">
                  This is the name in Pertisk. The Linux hostname is applied by cloud-init on first boot
                  (clone again after changing it).
                </p>
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
