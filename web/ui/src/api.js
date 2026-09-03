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

export async function api(path, opts = {}) {
  const headers = { ...(opts.headers || {}) }
  const token = getToken()
  if (token) headers.authorization = `Bearer ${token}`
  const isRaw =
    typeof FormData !== 'undefined' && opts.body instanceof FormData
      ? true
      : typeof Blob !== 'undefined' && opts.body instanceof Blob
  if (opts.body !== undefined && !isRaw) {
    headers['content-type'] = 'application/json'
  }
  const res = await fetch(path, {
    ...opts,
    headers,
    body:
      opts.body === undefined
        ? undefined
        : typeof opts.body === 'string' || isRaw
          ? opts.body
          : JSON.stringify(opts.body),
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
    throw err
  }
  return body
}

export function asList(value) {
  return Array.isArray(value) ? value : []
}

export function disksOf(vm) {
  return asList(vm?.spec?.disks)
}

export function netsOf(vm) {
  return asList(vm?.spec?.nets)
}

export function nicAddrs(nic) {
  return [nic?.ip, nic?.ipv6].filter(Boolean)
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
