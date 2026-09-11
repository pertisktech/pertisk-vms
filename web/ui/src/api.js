const TOKEN_KEY = 'pertisk_token'
const REMEMBER_KEY = 'pertisk_vm_remember'

export function getToken() {
  return localStorage.getItem(TOKEN_KEY) || sessionStorage.getItem(TOKEN_KEY) || ''
}

export function setToken(token, remember = true) {
  localStorage.setItem(REMEMBER_KEY, remember ? '1' : '0')
  if (remember) {
    localStorage.setItem(TOKEN_KEY, token)
    sessionStorage.removeItem(TOKEN_KEY)
  } else {
    sessionStorage.setItem(TOKEN_KEY, token)
    localStorage.removeItem(TOKEN_KEY)
  }
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY)
  sessionStorage.removeItem(TOKEN_KEY)
}

export function tokenIsRemembered() {
  return Boolean(localStorage.getItem(TOKEN_KEY))
}

const authListeners = new Set()
let authRequired = false

/** Called when the API returns 401. Does not clear the token or navigate. */
export function onAuthRequired(fn) {
  authListeners.add(fn)
  if (authRequired) fn()
  return () => authListeners.delete(fn)
}

export function isAuthRequired() {
  return authRequired
}

export function clearAuthRequired() {
  authRequired = false
}

export function isUnauthorized(err) {
  return Number(err?.status) === 401
}

function notifyAuthRequired() {
  authRequired = true
  for (const fn of [...authListeners]) fn()
}

export async function api(path, opts = {}) {
  const { notifyAuth = true, headers: extraHeaders, body: rawBody, ...fetchOpts } = opts
  const headers = { ...(extraHeaders || {}) }
  const token = getToken()
  if (token && path !== '/v1/login') headers.authorization = `Bearer ${token}`
  const isRaw =
    typeof FormData !== 'undefined' && rawBody instanceof FormData
      ? true
      : typeof Blob !== 'undefined' && rawBody instanceof Blob
  if (rawBody !== undefined && !isRaw) {
    headers['content-type'] = 'application/json'
  }
  const res = await fetch(path, {
    ...fetchOpts,
    headers,
    body:
      rawBody === undefined
        ? undefined
        : typeof rawBody === 'string' || isRaw
          ? rawBody
          : JSON.stringify(rawBody),
  })
  if (res.status === 204) return null
  const text = await res.text()
  let body = null
  if (text) {
    try {
      body = JSON.parse(text)
    } catch {
      body = { raw: text }
    }
  }
  if (!res.ok) {
    const err = new Error((body && body.error) || res.statusText || 'request failed')
    err.status = res.status
    if (res.status === 401 && path !== '/v1/login' && notifyAuth) notifyAuthRequired()
    throw err
  }
  return body
}

export function asList(value) {
  return Array.isArray(value) ? value : []
}

export function isTemplate(vm) {
  return Boolean(vm?.template)
}

/** Sidebar / crumb caption: `100 (web-01)`. */
export function vmCaption(vm) {
  const id = String(vm?.id ?? '')
  const name = String(vm?.spec?.name || '').trim()
  if (!id) return { id: '', name, title: name }
  if (!name || name === id) return { id, name: '', title: id }
  return { id, name, title: `${id} (${name})` }
}

export function nextVmId(vms) {
  const used = new Set((vms || []).map((vm) => String(vm.id)).filter((id) => /^\d{3,10}$/.test(id)))
  for (let id = 100; id <= 9_999_999_999; id += 1) {
    if (!used.has(String(id))) return String(id)
  }
  return ''
}

export function isCloudInitIso(name) {
  return String(name || '').toLowerCase().includes('cidata')
}

export function disksOf(vm) {
  return asList(vm?.spec?.disks)
}

export function netsOf(vm) {
  return asList(vm?.spec?.nets)
}

/** Public IPv6 — hide fe80:: link-local; prefer GUA over unique-local fd00::/8. */
export function publicIpv6(value) {
  const list = Array.isArray(value)
    ? value
    : value
      ? String(value).split(',')
      : []
  const addrs = list
    .map((ip) => String(ip || '').trim())
    .filter((s) => {
      const t = s.toLowerCase()
      return t && !t.startsWith('fe80:') && t !== '::1'
    })
  const gua = addrs.filter((s) => {
    const t = s.toLowerCase()
    return !t.startsWith('fc') && !t.startsWith('fd')
  })
  return gua.length ? gua : addrs
}

export function nicAddrs(nic) {
  const v6 = publicIpv6(nic?.ipv6)
  return [nic?.ip, ...v6].filter(Boolean)
}

export function replicasOf(vol) {
  return asList(vol?.replicas)
}

export function snapshotsOf(vol) {
  return asList(vol?.snapshots)
}

export function parseSize(raw) {
  const s = String(raw).trim().toUpperCase()
  const m = /^(\d+)([KMGT])?I?B?$/.exec(s)
  if (!m) throw new Error('Size like 8M or 1G')
  const n = Number(m[1])
  const mul = { K: 1024, M: 1024 ** 2, G: 1024 ** 3, T: 1024 ** 4 }[m[2]] || 1
  return n * mul
}

export function formatBytes(n) {
  const v = Number(n) || 0
  if (v < 1024) return `${v} B`
  const units = ['KiB', 'MiB', 'GiB', 'TiB']
  let x = v
  let i = -1
  do {
    x /= 1024
    i += 1
  } while (x >= 1024 && i < units.length - 1)
  return `${x >= 10 ? x.toFixed(0) : x.toFixed(1)} ${units[i]}`
}

/** Compact disk size for tables: 20GB, 50GB */
export function formatDiskGb(n) {
  const v = Number(n) || 0
  const gib = v / 1024 ** 3
  if (gib >= 1) {
    const rounded = gib >= 10 || Math.abs(gib - Math.round(gib)) < 0.05 ? Math.round(gib) : Math.round(gib * 10) / 10
    return `${rounded}GB`
  }
  const mib = v / 1024 ** 2
  if (mib >= 1) return `${Math.round(mib)}MB`
  return formatBytes(v)
}

export function formatUnix(sec) {
  const n = Number(sec)
  if (!n) return '—'
  const d = new Date(n * 1000)
  if (Number.isNaN(d.getTime())) return '—'
  return d.toLocaleString()
}

export function shortId(id) {
  const s = String(id || '')
  return s.length > 12 ? `${s.slice(0, 8)}…` : s
}

/** Host RAM kept for the daemon / OS. Matches daemon host_memory_reserve_mib. */
export function hostMemoryReserveMib(hostMib) {
  const host = Number(hostMib) || 0
  return Math.min(1536, Math.max(64, Math.floor(host / 20)))
}

/** Distro default SSH login inferred from a cloud image / template / volume name. */
const CLOUD_OS = [
  { re: /almalinux|alma[\s._-]?linux|\balma\b/, os: 'AlmaLinux', user: 'almalinux' },
  { re: /rocky/, os: 'Rocky Linux', user: 'rocky' },
  { re: /centos|cent[\s._-]?os/, os: 'CentOS', user: 'centos' },
  { re: /rhel|red[\s._-]?hat/, os: 'RHEL', user: 'cloud-user' },
  { re: /fedora/, os: 'Fedora', user: 'fedora' },
  { re: /debian/, os: 'Debian', user: 'debian' },
  { re: /ubuntu/, os: 'Ubuntu', user: 'ubuntu' },
  { re: /alpine/, os: 'Alpine', user: 'alpine' },
  { re: /oracle/, os: 'Oracle Linux', user: 'opc' },
  { re: /opensuse|sles|\bsuse\b/, os: 'openSUSE', user: 'opensuse' },
]

export function detectCloudOs(...hints) {
  const blob = hints.filter(Boolean).join(' ').toLowerCase()
  for (const item of CLOUD_OS) {
    if (item.re.test(blob)) return { os: item.os, user: item.user }
  }
  return { os: 'Ubuntu', user: 'ubuntu' }
}

export function defaultCloudUser(...hints) {
  return detectCloudOs(...hints).user
}

export function cloudInitHasLogin(password, sshKey) {
  return Boolean(String(password || '').trim() || String(sshKey || '').trim())
}

/** RAM a guest can be started with on this node. */
export function guestMemoryBudgetMib(cluster) {
  const members = cluster?.members || []
  const node = members.find((m) => m.online) || members[0]
  if (!node?.memory_mib) return null
  const reserve = hostMemoryReserveMib(node.memory_mib)
  const used = Number(node.used_memory_mib) || 0
  return Math.max(0, Number(node.memory_mib) - reserve - used)
}
