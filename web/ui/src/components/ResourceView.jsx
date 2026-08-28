import { NavLink, Outlet, useOutletContext } from 'react-router-dom'
import { Icon } from './Icons'

/// Proxmox-style resource panel: breadcrumb + action bar on top, vertical tab
/// strip on the left, tab body on the right.
export default function ResourceView({ icon, kind, name, status, tabs, actions }) {
  const ctx = useOutletContext()

  return (
    <div className="pve-panel">
      <div className="pve-toolbar">
        <div className="pve-crumb">
          <Icon name={icon} size={16} />
          <span className="pve-crumb-kind">{kind}</span>
          <span className="pve-crumb-sep">/</span>
          <strong>{name}</strong>
          {status}
        </div>
        <div className="pve-toolbar-actions">{actions}</div>
      </div>
      <div className="pve-panel-body">
        <nav className="pve-tabs" aria-label={`${kind} sections`}>
          {tabs.map((tab) => (
            <NavLink
              key={tab.to}
              to={tab.to}
              className={({ isActive }) => `pve-tab${isActive ? ' active' : ''}`}
            >
              <Icon name={tab.icon} size={15} />
              <span>{tab.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="pve-tabbody">
          <Outlet context={ctx} />
        </div>
      </div>
    </div>
  )
}
