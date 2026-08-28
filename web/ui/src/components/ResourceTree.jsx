import { useMemo, useState } from 'react'
import { NavLink } from 'react-router-dom'
import { asList } from '../api'
import { Icon } from './Icons'

const OPEN_KEY = 'pertisk_vm_tree_open'

function loadOpen() {
  try {
    const raw = JSON.parse(localStorage.getItem(OPEN_KEY) || '{}')
    return raw && typeof raw === 'object' ? raw : {}
  } catch {
    return {}
  }
}

function guestStatus(vm) {
  if (vm.state === 'running') return 'running'
  if (vm.state === 'failed') return 'failed'
  return 'stopped'
}

/// Nodes come from the cluster roster; a single-node daemon still reports itself.
function nodeList(cluster, host) {
  const members = asList(cluster?.members)
  if (members.length) return members
  return [{ id: 'local', name: host?.hostname || 'localhost', online: true }]
}

function Branch({ open, onToggle, icon, label, to, status, badge, depth, leaf }) {
  const body = (
    <>
      <span className="tree-twisty" onClick={leaf ? undefined : onToggle}>
        {!leaf && <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />}
      </span>
      <Icon name={icon} size={14} className="tree-icon" />
      <span className="tree-label">{label}</span>
      {status && <span className={`tree-dot ${status}`} />}
      {badge != null && <span className="tree-badge">{badge}</span>}
    </>
  )
  return (
    <NavLink
      to={to}
      className={({ isActive }) => `tree-row${isActive ? ' active' : ''}`}
      style={{ paddingLeft: `${0.4 + depth * 0.85}rem` }}
    >
      {body}
    </NavLink>
  )
}

export default function ResourceTree({ cluster, host, vms, volumes, networks, isos }) {
  const [open, setOpen] = useState(loadOpen)
  const [filter, setFilter] = useState('')

  function toggle(key) {
    setOpen((prev) => {
      const next = { ...prev, [key]: prev[key] === false }
      localStorage.setItem(OPEN_KEY, JSON.stringify(next))
      return next
    })
  }

  const isOpen = (key) => open[key] !== false
  const nodes = useMemo(() => nodeList(cluster, host), [cluster, host])
  const needle = filter.trim().toLowerCase()
  const match = (text) => !needle || String(text || '').toLowerCase().includes(needle)

  const guestsByNode = useMemo(() => {
    const map = new Map()
    for (const vm of vms) {
      const key = vm.node_id || nodes[0]?.id
      if (!map.has(key)) map.set(key, [])
      map.get(key).push(vm)
    }
    for (const list of map.values()) {
      list.sort((a, b) => (a.spec?.name || '').localeCompare(b.spec?.name || ''))
    }
    return map
  }, [vms, nodes])

  return (
    <div className="tree">
      <div className="tree-search">
        <Icon name="search" size={14} />
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Search"
          aria-label="Filter resources"
        />
        {filter && (
          <button type="button" onClick={() => setFilter('')} aria-label="Clear filter">
            <Icon name="x" size={13} />
          </button>
        )}
      </div>

      <div className="tree-scroll">
        <Branch
          depth={0}
          icon="datacenter"
          label={cluster?.name || 'Datacenter'}
          to="/dc/summary"
          open={isOpen('dc')}
          onToggle={() => toggle('dc')}
          status={cluster?.quorum === false ? 'failed' : undefined}
        />

        {isOpen('dc') && (
          <>
            {nodes.map((node) => {
              const guests = (guestsByNode.get(node.id) || []).filter((vm) =>
                match(vm.spec?.name || vm.id),
              )
              const nodeKey = `node:${node.id}`
              const showNode = match(node.name) || guests.length > 0
              if (!showNode) return null
              return (
                <div key={node.id}>
                  <Branch
                    depth={1}
                    icon="worker"
                    label={node.name}
                    to={`/node/${node.id}/summary`}
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
                        to={`/vm/${vm.id}/summary`}
                        status={guestStatus(vm)}
                      />
                    ))}
                </div>
              )
            })}

            <Branch
              depth={1}
              icon="folder"
              label="Storage"
              to="/dc/storage"
              open={isOpen('storage')}
              onToggle={() => toggle('storage')}
              badge={volumes.length + isos.length}
            />
            {isOpen('storage') && (
              <>
                {volumes
                  .filter((v) => match(v.name))
                  .map((vol) => (
                    <Branch
                      key={vol.id}
                      depth={2}
                      leaf
                      icon="disk"
                      label={vol.name}
                      to="/dc/storage"
                    />
                  ))}
                {isos
                  .filter((i) => match(i.name))
                  .map((iso) => (
                    <Branch
                      key={iso.name}
                      depth={2}
                      leaf
                      icon="volumes"
                      label={iso.name}
                      to="/dc/storage"
                    />
                  ))}
              </>
            )}

            <Branch
              depth={1}
              icon="folder"
              label="Networks"
              to="/dc/networks"
              open={isOpen('net')}
              onToggle={() => toggle('net')}
              badge={networks.length}
            />
            {isOpen('net') &&
              networks
                .filter((n) => match(n.name))
                .map((net) => (
                  <Branch
                    key={net.id}
                    depth={2}
                    leaf
                    icon="network"
                    label={net.name}
                    to="/dc/networks"
                  />
                ))}
          </>
        )}
      </div>
    </div>
  )
}
