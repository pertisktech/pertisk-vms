import { useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { api, asList, isTemplate } from '../api'
import { Btn } from '../components/Icons'
import { useConfirm } from '../components/Confirm'
import ResourceView from '../components/ResourceView'

export function useNode() {
  const { nodeId } = useParams()
  const ctx = useOutletContext()
  const members = asList(ctx.inv.cluster?.members)
  const node =
    members.find((m) => m.id === nodeId) ||
    (members.length === 0
      ? {
          id: nodeId,
          name: ctx.inv.host?.hostname || 'localhost',
          online: true,
          ipv4: ctx.inv.host?.ipv4 || [],
          ipv6: ctx.inv.host?.ipv6 || [],
        }
      : null)
  const guests = ctx.inv.vms.filter(
    (vm) =>
      !isTemplate(vm) && (vm.node_id === nodeId || (!vm.node_id && members.length === 0)),
  )
  return { ...ctx, nodeId, node, guests }
}

export default function NodeView() {
  const { nodeId, node, inv, canWrite } = useNode()
  const confirm = useConfirm()
  const nav = useNavigate()
  const members = asList(inv.cluster?.members)
  const self = !inv.cluster?.self_id || inv.cluster.self_id === nodeId
  const [busy, setBusy] = useState('')
  const [powerError, setPowerError] = useState('')

  async function power(kind) {
    const reboot = kind === 'reboot'
    const ok = await confirm({
      title: reboot ? 'Restart this node' : 'Shut down this node',
      message: reboot
        ? 'Reboot this hypervisor? Running guests get an ACPI shutdown first (then force stop). The UI disconnects until the node is back.'
        : 'Power off this hypervisor? Running guests get an ACPI shutdown first (then force stop). The node stays off until you press the power button.',
      confirmLabel: reboot ? 'Restart' : 'Shut down',
      tone: 'danger',
    })
    if (!ok) return
    setPowerError('')
    setBusy(kind)
    try {
      await api(`/v1/host/${kind}`, { method: 'POST' })
    } catch (err) {
      setBusy('')
      setPowerError(err.message || String(err))
    }
  }

  return (
    <ResourceView
      icon="worker"
      kind="Node"
      name={node?.name || nodeId}
      crumbs={['Datacenter', node?.name || nodeId]}
      status={
        <>
          <span className={`badge ${node?.online === false ? 'error' : 'ready'}`}>
            {node?.online === false ? 'offline' : 'online'}
          </span>
          {self && members.length > 1 && <span className="badge pending">this node</span>}
          {busy && (
            <span className="badge pending">
              {busy === 'reboot' ? 'restarting…' : 'shutting down…'}
            </span>
          )}
          {powerError && <span className="badge error">{powerError}</span>}
        </>
      }
      tabs={[
        { to: 'summary', label: 'Summary', icon: 'gauge' },
        { to: 'guests', label: 'Guests', icon: 'guests' },
        { to: 'updates', label: 'Updates', icon: 'updates' },
        { to: 'repositories', label: 'Repositories', icon: 'repo' },
        { to: 'shell', label: 'Console', icon: 'terminal' },
        { to: 'tasks', label: 'Task History', icon: 'activity' },
      ]}
      actions={
        <>
          <Btn
            icon="terminal"
            onClick={() => nav(`/node/${nodeId}/shell`)}
            title="Open node console"
          >
            Console
          </Btn>
          {canWrite && self && (
            <>
              <Btn
                icon="power"
                variant="secondary"
                disabled={!!busy}
                onClick={() => power('shutdown')}
                title="Power off this hypervisor"
              >
                Shutdown
              </Btn>
              <Btn
                icon="refresh"
                variant="secondary"
                disabled={!!busy}
                onClick={() => power('reboot')}
                title="Reboot this hypervisor"
              >
                Reboot
              </Btn>
            </>
          )}
        </>
      }
    />
  )
}
