import { useCallback, useEffect, useState } from 'react'
import { api, asList } from './api'

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

export function useInventory() {
  const [data, setData] = useState(EMPTY)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
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
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, 4000)
    return () => clearInterval(id)
  }, [refresh])

  const mutate = useCallback(
    async (fn) => {
      setError('')
      try {
        await fn()
        await refresh()
      } catch (err) {
        setError(err.message || String(err))
        throw err
      }
    },
    [refresh],
  )

  return { ...data, error, setError, loading, refresh, mutate }
}
