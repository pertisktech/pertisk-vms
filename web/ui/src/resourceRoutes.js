/** Valid tab segments per resource type (HashRouter paths). */
export const DC_TABS = ['summary', 'storage', 'networks', 'cluster', 'tasks', 'users']
export const NODE_TABS = ['summary', 'guests', 'tasks']
export const VM_TABS = ['summary', 'console', 'hardware', 'options']

export function parseResourceRoute(pathname) {
  const parts = pathname.split('/').filter(Boolean)
  if (parts[0] === 'dc') {
    return { type: 'dc', tab: parts[1] || 'summary' }
  }
  if (parts[0] === 'node' && parts[1]) {
    return { type: 'node', id: parts[1], tab: parts[2] || 'summary' }
  }
  if (parts[0] === 'vm' && parts[1]) {
    return { type: 'vm', id: parts[1], tab: parts[2] || 'summary' }
  }
  return { type: 'dc', tab: 'summary' }
}

function pickTab(requested, allowed, fallback = 'summary') {
  return allowed.includes(requested) ? requested : fallback
}

/** Build a sidebar link, keeping the current tab when switching within the same resource type. */
export function resourceLink(type, id, currentRoute) {
  if (type === 'dc') {
    const tab = currentRoute.type === 'dc' ? currentRoute.tab : 'summary'
    return `/dc/${pickTab(tab, DC_TABS)}`
  }
  if (type === 'node') {
    const tab =
      currentRoute.type === 'node'
        ? currentRoute.tab
        : currentRoute.type === 'vm'
          ? 'guests'
          : 'summary'
    return `/node/${id}/${pickTab(tab, NODE_TABS)}`
  }
  if (type === 'vm') {
    const tab = currentRoute.type === 'vm' ? currentRoute.tab : 'summary'
    return `/vm/${id}/${pickTab(tab, VM_TABS)}`
  }
  return '/dc/summary'
}
