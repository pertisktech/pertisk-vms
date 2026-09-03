import { useCallback, useEffect, useState } from 'react'
import { api } from './api'

/**
 * Poll live metrics.
 * @param {'cluster'|'node'|string} scope - 'cluster' | 'node' | vm id
 */
export function useMetrics(scope) {
  const [data, setData] = useState(null)
  const [error, setError] = useState('')

  const refresh = useCallback(async () => {
    if (scope == null || scope === '') return
    try {
      let path = '/v1/metrics'
      if (scope === 'node') path = '/v1/metrics/node'
      else if (scope !== 'cluster') path = `/v1/vms/${scope}/metrics`
      const next = await api(path)
      setData(next)
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    }
  }, [scope])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, 4000)
    return () => clearInterval(id)
  }, [refresh])

  return { data, error, refresh }
}
