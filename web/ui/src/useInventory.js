import { useCallback, useEffect, useState } from 'react'
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

export function useInventory() {
  const [data, setData] = useState(EMPTY)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    if (isAuthRequired()) {
      setLoading(false)
      return
    }
    try {
      const [host, cluster, vms, volumes, isos, networks, tasks, audit] = await Promise.all([
        api('/v1/host'),
        api('/v1/cluster'),
        api('/v1/vms'),
        api('/v1/volumes'),
        api('/v1/isos'),
        api('/v1/networks'),
        api('/v1/tasks'),
        api('/v1/audit'),
      ])
      setData({
        host: host || null,
        cluster: cluster || EMPTY.cluster,
        vms: asList(vms),
        volumes: asList(volumes),
        isos: asList(isos),
        networks: asList(networks),
        tasks: asList(tasks),
        audit: asList(audit),
      })
      setError('')
      requestRefresh()
    } catch (err) {
      if (isUnauthorized(err)) return
      setError(err.message || String(err))
    } finally {
      setLoading(false)
    }
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
