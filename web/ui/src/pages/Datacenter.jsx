import { useOutletContext } from 'react-router-dom'
import ResourceView from '../components/ResourceView'

export default function Datacenter() {
  const { user, inv } = useOutletContext()
  const tabs = [
    { to: 'summary', label: 'Summary', icon: 'summary' },
    { to: 'storage', label: 'Storage', icon: 'disk' },
    { to: 'networks', label: 'Networks', icon: 'network' },
    { to: 'cluster', label: 'Cluster', icon: 'cluster' },
    { to: 'tasks', label: 'Task History', icon: 'activity' },
  ]
  if (user?.role === 'admin') {
    tabs.push({ to: 'users', label: 'Permissions', icon: 'users' })
  }

  return (
    <ResourceView
      icon="datacenter"
      kind="Datacenter"
      name={inv.cluster?.name || 'pertisk'}
      status={
        inv.cluster?.fenced ? <span className="badge error">fenced</span> : null
      }
      tabs={tabs}
    />
  )
}
