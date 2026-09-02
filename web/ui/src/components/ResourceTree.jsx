import { useMemo, useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'
import { asList } from '../api'
import { Icon } from './Icons'
import { parseResourceRoute, resourceLink } from '../resourceRoutes'

const OPEN_KEY = 'pertisk_vm_tree_open'

function loadOpen() {
  try {
    const raw = JSON.parse(localStorage.getItem(OPEN_KEY) || '{}')
    return raw && typeof raw === 'object' ? raw : {}
  } catch {
    return {}
  }
}

/// Nodes come from the cluster roster; a single-node daemon still reports itself.
function nodeList(cluster, host) {
  const members = asList(cluster?.members)
  if (members.length) return members
  return [{ id: 'local', name: host?.hostname || 'localhost', online: true }]
}

function guestStatus(vm) {
  if (vm.state === 'running') return 'running'
  if (vm.state === 'failed') return 'failed'
  return 'stopped'
}

function Branch({ open, onToggle, icon, label, to, status, badge, depth, leaf }) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) => `tree-row${isActive ? ' active' : ''}`}
      style={{ paddingLeft: `${0.4 + depth * 0.85}rem` }}
    >
      <span
        className="tree-twisty"
        role={leaf ? undefined : 'button'}
        aria-label={leaf ? undefined : open ? 'Collapse' : 'Expand'}
        onClick={
          leaf
            ? undefined
            : (e) => {
                e.preventDefault()
                e.stopPropagation()
                onToggle()
              }
        }
      >
        {!leaf && <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />}
      </span>
      <Icon name={icon} size={14} className="tree-icon" />
      <span className="tree-label">{label}</span>
      {status && <span className={`tree-dot ${status}`} />}
      {badge != null && <span className="tree-badge">{badge}</span>}
    </NavLink>
  )
}

export default function ResourceTree({ cluster, host, vms }) {
  const location = useLocation()
  const currentRoute = useMemo(() => parseResourceRoute(location.pathname), [location.pathname])
  const [open, setOpen] = useState(loadOpen)

  function toggle(key) {
    setOpen((prev) => {
      const next = { ...prev, [key]: prev[key] === false }
      localStorage.setItem(OPEN_KEY, JSON.stringify(next))
      return next
    })
  }

  const isOpen = (key) => open[key] !== false
  const nodes = useMemo(() => nodeList(cluster, host), [cluster, host])
  const guestsByNode = useMemo(() => {
    const guests = new Map()
    for (const vm of vms) {
      const nodeId = vm.node_id || nodes[0]?.id
      if (!guests.has(nodeId)) guests.set(nodeId, [])
      guests.get(nodeId).push(vm)
    }
    for (const nodeGuests of guests.values()) {
      nodeGuests.sort((left, right) => (left.spec?.name || '').localeCompare(right.spec?.name || ''))
    }
    return guests
  }, [vms, nodes])

  return (
    <div className="tree">
      <div className="tree-scroll">
        <Branch
          depth={0}
          icon="datacenter"
          label="Pertisk"
          to={resourceLink('dc', null, currentRoute)}
          open={isOpen('dc')}
          onToggle={() => toggle('dc')}
          status={cluster?.quorum === false ? 'failed' : undefined}
        />

        {isOpen('dc') &&
          nodes.map((node) => {
            const guests = guestsByNode.get(node.id) || []
            const nodeKey = `node:${node.id}`
            return (
              <div key={node.id}>
                <Branch
                  depth={1}
                  icon="worker"
                  label={node.name}
                  to={resourceLink('node', node.id, currentRoute)}
                  open={isOpen(nodeKey)}
                  onToggle={() => toggle(nodeKey)}
                  status={node.online ? 'running' : 'failed'}
                />
                {isOpen(nodeKey) &&
                  guests.map((vm) => (
                    <Branch
                      key={vm.id}
                      depth={2}
                      leaf
                      icon="guests"
                      label={vm.spec?.name || vm.id}
                      to={resourceLink('vm', vm.id, currentRoute)}
                      status={guestStatus(vm)}
                    />
                  ))}
              </div>
            )
          })}
      </div>
    </div>
  )
}
