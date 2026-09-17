import { useMemo, useState } from 'react'
import { api, asList, isTemplate, shortId } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useConfirm } from '../../components/Confirm'
import { useGuest } from '../GuestView'

function nodeName(cluster, host, id) {
  const members = asList(cluster?.members)
  return members.find((m) => m.id === id)?.name || host?.hostname || (id ? shortId(id) : '—')
}

export default function GuestOptions() {
  const { vm, canWrite, inv } = useGuest()
  const confirm = useConfirm()
  const [selected, setSelected] = useState(null)
  const [dialog, setDialog] = useState(null)
  const [form, setForm] = useState({})
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const template = isTemplate(vm)
  const running = vm?.state === 'running'

  const rows = useMemo(() => {
    const editable = canWrite
    return [
      {
        key: 'name',
        label: 'Name',
        value: vm.spec?.name || '—',
        edit: 'name',
        editable,
      },
      {
        key: 'state',
        label: 'State',
        value: template ? 'template' : vm.state || '—',
      },
      {
        key: 'node',
        label: 'Node',
        value: nodeName(inv.cluster, inv.host, vm.node_id),
      },
      {
        key: 'vcpus',
        label: 'vCPUs',
        value: String(vm.spec?.vcpus || 1),
      },
      {
        key: 'memory',
        label: 'Memory',
        value: `${vm.spec?.memory_mib || 0} MiB`,
      },
      {
        key: 'console',
        label: 'Console',
        value: vm.spec?.console_type || 'serial',
      },
      {
        key: 'ha',
        label: 'High Availability',
        value: vm.spec?.ha !== false ? 'restart on node loss' : 'off',
        edit: 'ha',
        editable: editable && !template,
      },
      {
        key: 'autostart',
        label: 'Start at boot',
        value: vm.spec?.autostart ? 'yes' : 'no',
        edit: 'autostart',
        editable: editable && !template,
      },
      {
        key: 'autostart-order',
        label: 'Start order',
        value: String(vm.spec?.autostart_order || 0),
        edit: 'autostart',
        editable: editable && !template,
      },
      {
        key: 'autostart-delay',
        label: 'Startup delay',
        value: `${vm.spec?.autostart_delay || 0} s`,
        edit: 'autostart',
        editable: editable && !template,
      },
      {
        key: 'disks',
        label: 'Disks',
        value: String((vm.spec?.disks || []).filter((d) => !d.cdrom).length),
      },
      {
        key: 'id',
        label: 'Guest ID',
        value: String(vm.id),
      },
    ]
  }, [vm, canWrite, template, inv.cluster, inv.host])

  const selectedRow = rows.find((r) => r.key === selected) || null

  function openDialog(kind) {
    if (!kind) return
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

  function editSelected() {
    if (!selectedRow?.editable || !selectedRow.edit) return
    openDialog(selectedRow.edit)
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

  return (
    <div className="pve-tab-page">
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

      <section className="pve-options-panel">
        <div className="pve-options-head" role="row">
          <span>Name</span>
          <span>Value</span>
          <span className="sr-only">Actions</span>
        </div>
        {rows.map((row) => {
          const active = selected === row.key
          return (
            <button
              type="button"
              key={row.key}
              role="row"
              className={`pve-options-row ${active ? 'active' : ''}`}
              onClick={() => setSelected(row.key)}
              onDoubleClick={() => row.editable && openDialog(row.edit)}
            >
              <span className="pve-options-name">{row.label}</span>
              <span className="pve-options-value mono-inline">{row.value}</span>
              <span className={`pve-options-edit ${row.editable && active ? 'show' : ''}`}>
                {row.editable ? <Icon name="pencil" size={14} /> : null}
              </span>
            </button>
          )
        })}
        <footer className="pve-options-foot">
          <Btn
            icon="pencil"
            disabled={!selectedRow?.editable}
            onClick={editSelected}
          >
            Edit
          </Btn>
          <Btn
            icon="x"
            variant="secondary"
            disabled={!selected}
            onClick={() => setSelected(null)}
          >
            Clear
          </Btn>
          <span className="pve-options-sync muted">
            <Icon name="check" size={14} />
            Configuration in sync
          </span>
        </footer>
      </section>

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
                  <small>
                    {inv.host?.ha_durable || inv.host?.storage_backend === 'rbd'
                      ? 'High availability (Ceph RBD)'
                      : 'Restart only — replica storage can lose unsynced writes'}
                  </small>
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
