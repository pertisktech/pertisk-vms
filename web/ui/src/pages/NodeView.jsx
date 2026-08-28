import { useOutletContext, useParams } from 'react-router-dom'
import { asList } from '../api'
import ResourceView from '../components/ResourceView'

export function useNode() {
  const { nodeId } = useParams()
  const ctx = useOutletContext()
  const members = asList(ctx.inv.cluster?.members)
  const node =
    members.find((m) => m.id === nodeId) ||
    (members.length === 0
      ? { id: nodeId, name: ctx.inv.host?.hostname || 'localhost', online: true }
      : null)
  const guests = ctx.inv.vms.filter(
    (vm) => vm.node_id === nodeId || (!vm.node_id && members.length === 0),
  )
  return { ...ctx, nodeId, node, guests }
}

export default function NodeView() {
  const { nodeId, node, inv } = useNode()
  const members = asList(inv.cluster?.members)
  const self = inv.cluster?.self_id === nodeId

  return (
    <ResourceView
      icon="worker"
      kind="Node"
      name={node?.name || nodeId}
      status={
        <>
          <span className={`badge ${node?.online === false ? 'error' : 'ready'}`}>
            {node?.online === false ? 'offline' : 'online'}
          </span>
          {self && members.length > 1 && <span className="badge pending">this node</span>}
        </>
      }
      tabs={[
        { to: 'summary', label: 'Summary', icon: 'summary' },
        { to: 'guests', label: 'Guests', icon: 'guests' },
        { to: 'tasks', label: 'Task History', icon: 'activity' },
      ]}
    />
  )
}
