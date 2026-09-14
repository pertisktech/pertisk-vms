import { useEffect, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { api, clearToken, isTemplate } from '../../api'
import { ThemeProvider } from '../../ThemeContext'
import { GuestProvider } from '../GuestView'
import GuestSsh from './Ssh'

/** Standalone SSH window — no datacenter chrome. */
export default function GuestSshPopup() {
  return (
    <ThemeProvider>
      <GuestSshPopupInner />
    </ThemeProvider>
  )
}

function GuestSshPopupInner() {
  const { vmId } = useParams()
  const nav = useNavigate()
  const [vm, setVm] = useState(null)
  const [role, setRole] = useState('')
  const [error, setError] = useState('')

  useEffect(() => {
    document.title = `SSH · ${vm?.spec?.name || vmId || 'guest'}`
  }, [vm, vmId])

  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const [guest, session] = await Promise.all([api(`/v1/vms/${vmId}`), api('/v1/session')])
        if (cancelled) return
        if (isTemplate(guest)) {
          setError('Templates have no SSH session')
          return
        }
        setVm(guest)
        setRole(session?.role || '')
      } catch (err) {
        if (cancelled) return
        if (String(err?.message || err).includes('401')) {
          clearToken()
          nav('/login', { replace: true })
          return
        }
        setError(String(err?.message || err))
      }
    }
    load()
    const t = window.setInterval(load, 8000)
    return () => {
      cancelled = true
      window.clearInterval(t)
    }
  }, [vmId, nav])

  const value = useMemo(
    () => ({
      vmId,
      vm,
      canWrite: role === 'admin' || role === 'operator',
      inv: { vms: vm ? [vm] : [], volumes: [], networks: [], cluster: null, host: null },
    }),
    [vmId, vm, role],
  )

  if (error) {
    return (
      <div className="pve-ssh-window">
        <div className="dash-empty card">
          <strong>SSH</strong>
          <p className="muted">{error}</p>
        </div>
      </div>
    )
  }
  if (!vm) {
    return (
      <div className="pve-ssh-window">
        <p className="muted" style={{ padding: '1rem' }}>
          Loading…
        </p>
      </div>
    )
  }

  return (
    <div className="pve-ssh-window">
      <GuestProvider value={value}>
        <GuestSsh popup />
      </GuestProvider>
    </div>
  )
}
