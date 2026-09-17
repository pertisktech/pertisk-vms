import { useCallback, useEffect, useState } from 'react'
import { api, asList } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useConfirm } from '../../components/Confirm'
import { useNode } from '../NodeView'

export default function NodeUpdates() {
  const { canWrite, inv } = useNode()
  const confirm = useConfirm()
  const [status, setStatus] = useState(null)
  const [error, setError] = useState('')
  const [log, setLog] = useState('')
  const [busy, setBusy] = useState('')

  const load = useCallback(async () => {
    try {
      setStatus(await api('/v1/updates'))
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  function applyLog(result) {
    const text = typeof result?.log === 'string' ? result.log.trim() : ''
    setLog(text || 'Finished.')
  }

  async function refresh() {
    setBusy('refresh')
    setError('')
    setLog('Refreshing package lists…')
    try {
      const result = await api('/v1/updates/refresh', { method: 'POST' })
      applyLog(result)
      await load()
      inv.refresh()
    } catch (err) {
      setLog(err.message || String(err))
    } finally {
      setBusy('')
    }
  }

  async function upgrade() {
    const ok = await confirm({
      title: 'Upgrade this node',
      message:
        'Install available host packages on this hypervisor (apt or dnf). Guests stay on disk. A kernel update needs a reboot afterwards.',
      confirmLabel: 'Upgrade',
      tone: 'danger',
    })
    if (!ok) return
    setBusy('upgrade')
    setError('')
    setLog('Installing updates…')
    try {
      const result = await api('/v1/updates/upgrade', { method: 'POST' })
      applyLog(result)
      await load()
      inv.refresh()
    } catch (err) {
      setLog(err.message || String(err))
    } finally {
      setBusy('')
    }
  }

  async function rebootHost() {
    const ok = await confirm({
      title: 'Restart this node',
      message:
        'Reboot this hypervisor to finish the kernel or firmware update? Running guests get an ACPI shutdown first. The UI disconnects until the node is back.',
      confirmLabel: 'Restart',
      tone: 'danger',
    })
    if (!ok) return
    setBusy('reboot')
    try {
      await api('/v1/host/reboot', { method: 'POST' })
    } catch (err) {
      setError(err.message || String(err))
      setBusy('')
    }
  }

  const packages = asList(status?.packages)
  const ready = status?.apt !== false

  return (
    <div className="dash-page updates-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="updates" size={20} />
            Updates
          </h1>
          <p className="dash-lead muted">In-place host upgrades (apt / dnf). Guests are not reflashed.</p>
        </div>
        {canWrite && (
          <div className="dash-resources-actions">
            <Btn icon="refresh" variant="secondary" disabled={!!busy || !ready} onClick={refresh}>
              {busy === 'refresh' ? 'Refreshing…' : 'Refresh'}
            </Btn>
            <Btn icon="updates" disabled={!!busy || !ready} onClick={upgrade}>
              {busy === 'upgrade' ? 'Upgrading…' : 'Upgrade'}
            </Btn>
            {status?.reboot_required && (
              <Btn icon="refresh" variant="secondary" disabled={!!busy} onClick={rebootHost}>
                {busy === 'reboot' ? 'Restarting…' : 'Restart node'}
              </Btn>
            )}
          </div>
        )}
      </div>

      <div className="updates-body">
        {error && (
          <div className="banner danger">
            {error}
            <button type="button" className="banner-dismiss" onClick={() => setError('')}>
              ×
            </button>
          </div>
        )}
        {busy === 'reboot' && <div className="banner">Restarting node…</div>}
        {status?.reboot_required && !busy && (
          <div className="banner">A reboot is required to finish a kernel or firmware update.</div>
        )}
        {!ready && (
          <div className="dash-empty card">
            <strong>No package manager on this node</strong>
            <p className="muted">{status?.reason || 'Need apt-get (Debian) or dnf (AlmaLinux).'}</p>
          </div>
        )}
        {ready && status?.reason && <div className="banner danger">{status.reason}</div>}
        {ready && (
          <section className="card table-card updates-table">
            <div className="table-meta">
              {packages.length
                ? `${packages.length} package${packages.length === 1 ? '' : 's'} to upgrade`
                : 'Already up to date'}
            </div>
            {packages.length === 0 ? (
              <p className="muted">Refresh to check enabled repositories (Debian apt or AlmaLinux dnf).</p>
            ) : (
              <div className="table-shell">
                <table>
                  <thead>
                    <tr>
                      <th>Package</th>
                      <th>Version</th>
                      <th>Available</th>
                      <th>Origin</th>
                    </tr>
                  </thead>
                  <tbody>
                    {packages.map((pkg) => (
                      <tr key={`${pkg.name}:${pkg.arch || ''}:${pkg.available || ''}`}>
                        <td>
                          {pkg.name}
                          {pkg.arch ? <span className="muted">:{pkg.arch}</span> : null}
                        </td>
                        <td className="mono-inline">{pkg.version || '—'}</td>
                        <td className="mono-inline">{pkg.available || '—'}</td>
                        <td className="muted">{pkg.origin || '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        )}
        <section className="updates-log-card">
          <div className="pve-console-bar">
            <span className="console-traffic" aria-hidden>
              <span />
              <span />
              <span />
            </span>
            <span>Output</span>
          </div>
          <pre className="update-log">
            {log || 'Refresh or Upgrade to see package-manager output here.'}
          </pre>
        </section>
      </div>
    </div>
  )
}
