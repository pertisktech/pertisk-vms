import { NavLink, Outlet, useOutletContext } from 'react-router-dom'
import { Icon } from './Icons'

export default function ResourceView({ icon, kind, name, crumbs, status, tabs, actions }) {
  const ctx = useOutletContext()
  const trail = crumbs?.length ? crumbs : [kind, name].filter(Boolean)

  return (
    <div className="pve-panel">
      <div className="pve-toolbar">
        <nav className="pve-crumbs" aria-label="Breadcrumb">
          {trail.map((item, i) => (
            <span key={`${item}-${i}`} className="flex-crumb">
              {i > 0 && <Icon name="chevron-right" size={12} />}
              <span className={i === trail.length - 1 ? 'current' : undefined}>{item}</span>
            </span>
          ))}
        </nav>
        <div className="pve-title-row">
          <span className="pve-title-icon" aria-hidden>
            <Icon name={icon} size={18} />
          </span>
          <div className="pve-title-copy">
            <h2>{name}</h2>
            {status ? <div className="pve-title-status">{status}</div> : null}
          </div>
          <div className="pve-toolbar-actions">{actions}</div>
        </div>
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
