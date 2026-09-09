import { getToken } from './api'

const listeners = new Set()
const metricCounts = new Map()
let socket = null
let retainers = 0
let wanted = false
let reconnectTimer = null
let reconnectDelay = 500

function eventsUrl() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${location.host}/v1/events/ws?token=${encodeURIComponent(getToken())}`
}

function dispatch(msg) {
  for (const listener of listeners) listener(msg)
}

function send(payload) {
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify(payload))
  }
}

function syncWatch() {
  send({ type: 'subscribe', metrics: [...metricCounts.keys()] })
}

function scheduleReconnect() {
  if (!wanted) return
  clearTimeout(reconnectTimer)
  reconnectTimer = setTimeout(() => {
    reconnectDelay = Math.min(reconnectDelay * 2, 8000)
    open()
  }, reconnectDelay)
}

function open() {
  if (!wanted) return
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return
  }
  clearTimeout(reconnectTimer)
  try {
    socket = new WebSocket(eventsUrl())
  } catch {
    scheduleReconnect()
    return
  }
  socket.onopen = () => {
    reconnectDelay = 500
    dispatch({ type: '_open' })
    syncWatch()
    send({ type: 'refresh' })
  }
  socket.onmessage = (event) => {
    try {
      dispatch(JSON.parse(event.data))
    } catch {
      /* ignore malformed frames */
    }
  }
  socket.onclose = () => {
    socket = null
    dispatch({ type: '_close' })
    scheduleReconnect()
  }
  socket.onerror = () => {
    socket?.close()
  }
}

export function retainLive(onMessage) {
  retainers += 1
  wanted = true
  listeners.add(onMessage)
  open()
  return () => {
    listeners.delete(onMessage)
    retainers -= 1
    if (retainers <= 0) {
      retainers = 0
      wanted = false
      clearTimeout(reconnectTimer)
      socket?.close()
      socket = null
    }
  }
}

export function watchMetrics(scope) {
  if (scope == null || scope === '') return () => {}
  const key = String(scope)
  metricCounts.set(key, (metricCounts.get(key) || 0) + 1)
  syncWatch()
  return () => {
    const next = (metricCounts.get(key) || 1) - 1
    if (next <= 0) metricCounts.delete(key)
    else metricCounts.set(key, next)
    syncWatch()
  }
}

export function requestRefresh() {
  send({ type: 'refresh' })
}
