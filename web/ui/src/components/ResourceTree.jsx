import { useMemo, useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'
import { asList, isTemplate, vmCaption } from '../api'
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
  if (isTemplate(vm)) return undefined
  if (vm.state === 'running') return 'running'
  if (vm.state === 'failed') return 'failed'
  return 'stopped'
}

function compareVmId(left, right) {
  const a = Number(left.id)
  const b = Number(right.id)
  if (Number.isFinite(a) && Number.isFinite(b) && a !== b) return a - b
  return String(left.id).localeCompare(String(right.id), undefined, { numeric: true })
}

function GuestLabel({ vm }) {
  const { id, name } = vmCaption(vm)
  return (
    <>
      <span className="tree-vmid">{id}</span>
      {name ? <span className="tree-vmname"> ({name})</span> : null}
    </>
  )
}

function Branch({ open, onToggle, icon, label, title, to, status, badge, depth, leaf }) {
  const titleText = title || (typeof label === 'string' ? label : undefined)
  return (
    <NavLink
      to={to}
      className={({ isActive }) => `tree-row${isActive ? ' active' : ''}${leaf ? ' leaf' : ''}`}
      style={{ paddingLeft: `${0.4 + depth * 0.85}rem` }}
      title={titleText}
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
      nodeGuests.sort(compareVmId)
    }
    return guests
  }, [vms, nodes])

  return (
    <div className="tree">
      <div className="tree-scroll">
        <Branch
          depth={0}
          icon="datacenter"
          label={cluster?.name || 'pertisk'}
          title={cluster?.name ? `Cluster ${cluster.name}` : 'Cluster'}
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
                  guests.map((vm) => {
                    const caption = vmCaption(vm)
                    return (
                      <Branch
                        key={vm.id}
                        depth={2}
                        leaf
                        icon={isTemplate(vm) ? 'template' : 'guests'}
                        label={<GuestLabel vm={vm} />}
                        title={caption.title}
                        to={resourceLink('vm', vm.id, currentRoute, { template: isTemplate(vm) })}
                        status={guestStatus(vm)}
                        badge={isTemplate(vm) ? 'tpl' : undefined}
                      />
                    )
                  })}
              </div>
            )
          })}
      </div>
    </div>
  )
}
