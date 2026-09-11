import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import { retainLive, watchMetrics } from './live'

const MAX_POINTS = 60

function formatTimeOnly(date) {
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function pct(used, total) {
  const u = Number(used) || 0
  const t = Number(total) || 0
  if (t <= 0) return 0
  return Math.min(100, Math.round((u / t) * 1000) / 10)
}

function toPoint(live) {
  const t = live?.collected_at_ms ? Number(live.collected_at_ms) : Date.now()
  return {
    t,
    collected_at_ms: t,
    time: formatTimeOnly(new Date(t)),
    cpu: Number(live?.cpu_pct) || 0,
    mem_used: Number(live?.mem_used_bytes) || 0,
    mem_total: Number(live?.mem_total_bytes) || 0,
    mem_pct: pct(live?.mem_used_bytes, live?.mem_total_bytes),
    disk_used: Number(live?.disk_used_bytes) || 0,
    disk_total: Number(live?.disk_total_bytes) || 0,
    disk_pct: pct(live?.disk_used_bytes, live?.disk_total_bytes),
    rx: Number(live?.net_rx_bps) || 0,
    tx: Number(live?.net_tx_bps) || 0,
    rx_mibps: (Number(live?.net_rx_bps) || 0) / (1024 * 1024),
    tx_mibps: (Number(live?.net_tx_bps) || 0) / (1024 * 1024),
  }
}

function applySample(next, setData, setHistory) {
  setData(next)
  const sample = next?.live
  if (!sample) return
  const point = toPoint(sample)
  setHistory((prev) => {
    const last = prev[prev.length - 1]
    if (last && last.collected_at_ms === point.collected_at_ms) return prev
    return [...prev, point].slice(-MAX_POINTS)
  })
}

/**
 * Live metrics over the shared events websocket, with a REST snapshot for first paint.
 * @param {'cluster'|'node'|string} scope - 'cluster' | 'node' | vm id
 */
export function useMetrics(scope) {
  const [data, setData] = useState(null)
  const [history, setHistory] = useState([])
  const [error, setError] = useState('')
  const [live, setLive] = useState(true)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(
    async ({ silent = false } = {}) => {
      if (scope == null || scope === '') return
      if (!silent) setLoading(true)
      try {
        let path = '/v1/metrics'
        if (scope === 'node') path = '/v1/metrics/node'
        else if (scope !== 'cluster') path = `/v1/vms/${scope}/metrics`
        const next = await api(path)
        applySample(next, setData, setHistory)
        setError('')
      } catch (err) {
        if (err.status === 401) return
        setError(err.message || String(err))
      } finally {
        if (!silent) setLoading(false)
      }
    },
    [scope],
  )

  useEffect(() => {
    setData(null)
    setHistory([])
    setError('')
    refresh()
  }, [refresh])

  useEffect(() => {
    if (scope == null || scope === '') return undefined
    return retainLive((msg) => {
      if (msg.type !== 'metrics' || String(msg.scope) !== String(scope)) return
      applySample(msg.data, setData, setHistory)
      setError('')
      setLoading(false)
    })
  }, [scope])

  useEffect(() => {
    if (!live || scope == null || scope === '') return undefined
    return watchMetrics(scope)
  }, [live, scope])

  return { data, history, error, refresh, live, setLive, loading }
}
