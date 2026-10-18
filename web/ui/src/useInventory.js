import { useCallback, useEffect, useRef, useState } from 'react'
import { api, asList, isAuthRequired, isUnauthorized } from './api'
import { requestRefresh, retainLive } from './live'

const EMPTY = {
  host: null,
  cluster: { name: '', members: [], quorum: false, fenced: false, generation: 0 },
  vms: [],
  volumes: [],
  isos: [],
  networks: [],
  tasks: [],
  audit: [],
}

function fromInventory(msg) {
  return {
    host: msg.host || null,
    cluster: msg.cluster || EMPTY.cluster,
    vms: asList(msg.vms),
    volumes: asList(msg.volumes),
    isos: asList(msg.isos),
    networks: asList(msg.networks),
    tasks: asList(msg.tasks),
    audit: asList(msg.audit),
  }
}

function piece(key, value) {
  if (key === 'host') return value || null
  if (key === 'cluster') return value || EMPTY.cluster
  return asList(value)
}

export function useInventory() {
  const [data, setData] = useState(EMPTY)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const refreshSeq = useRef(0)

  const refresh = useCallback(async () => {
    if (isAuthRequired()) {
      setLoading(false)
      return
    }
    const mine = ++refreshSeq.current
    const jobs = [
      ['host', '/v1/host'],
      ['cluster', '/v1/cluster'],
      ['vms', '/v1/vms'],
      ['volumes', '/v1/volumes'],
      ['isos', '/v1/isos'],
      ['networks', '/v1/networks'],
      ['tasks', '/v1/tasks'],
      ['audit', '/v1/audit'],
    ]
    let failed = ''
    let ok = false
    await Promise.all(
      jobs.map(async ([key, path]) => {
        try {
          const value = await api(path)
          if (mine !== refreshSeq.current) return
          ok = true
          setData((prev) => ({ ...prev, [key]: piece(key, value) }))
        } catch (err) {
          if (mine !== refreshSeq.current || isUnauthorized(err)) return
          if (!failed) failed = err?.message || String(err)
        }
      }),
    )
    if (mine !== refreshSeq.current) return
    if (ok && !failed) setError('')
    if (failed) setError(failed)
    if (ok) requestRefresh()
    setLoading(false)
  }, [])

  useEffect(() => {
    refresh()
    return retainLive((msg) => {
      if (msg.type !== 'inventory') return
      setData(fromInventory(msg))
      setError('')
      setLoading(false)
    })
  }, [refresh])

  const mutate = useCallback(
    async (fn) => {
      setError('')
      try {
        await fn()
        await refresh()
      } catch (err) {
        if (!isUnauthorized(err)) setError(err.message || String(err))
        throw err
      }
    },
    [refresh],
  )

  return { ...data, error, setError, loading, refresh, mutate }
}
