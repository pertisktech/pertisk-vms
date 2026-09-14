import { useEffect, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api } from '../api'
import { Btn, Icon } from '../components/Icons'

const ALL_EVENTS = [
  { id: 'vm.create', label: 'VM created' },
  { id: 'vm.destroy', label: 'VM destroyed' },
  { id: 'vm.start', label: 'VM started' },
  { id: 'vm.stop', label: 'VM stopped' },
  { id: 'vm.failed', label: 'VM failed' },
  { id: 'task.error', label: 'Task error' },
  { id: 'node.offline', label: 'Node offline' },
]

const EMPTY = {
  node_name: '',
  enabled: false,
  smtp_host: '',
  smtp_port: 587,
  smtp_tls: 'starttls',
  smtp_user: '',
  smtp_password: '',
  smtp_password_set: false,
  from: '',
  recipients: '',
  events: [],
}

function fromApi(data) {
  const n = data?.notify || {}
  return {
    node_name: data?.node_name || '',
    enabled: Boolean(n.enabled),
    smtp_host: n.smtp_host || '',
    smtp_port: Number(n.smtp_port) || 587,
    smtp_tls: n.smtp_tls || 'starttls',
    smtp_user: n.smtp_user || '',
    smtp_password: '',
    smtp_password_set: Boolean(n.smtp_password_set),
    from: n.from || '',
    recipients: (n.recipients || []).join('\n'),
    events: Array.isArray(n.events) ? [...n.events] : [],
  }
}

export default function Settings() {
  const { canWrite } = useOutletContext()
  const [form, setForm] = useState(EMPTY)
  const [error, setError] = useState('')
  const [ok, setOk] = useState('')
  const [busy, setBusy] = useState(false)
  const [loaded, setLoaded] = useState(false)

  async function refresh() {
    try {
      setForm(fromApi(await api('/v1/settings')))
      setError('')
      setLoaded(true)
    } catch (err) {
      if (err.status !== 401) setError(err.message || String(err))
    }
  }

  useEffect(() => {
    refresh()
  }, [])

  function setField(key, value) {
    setForm((prev) => ({ ...prev, [key]: value }))
  }

  function toggleEvent(id) {
    setForm((prev) => {
      const has = prev.events.includes(id)
      return {
        ...prev,
        events: has ? prev.events.filter((e) => e !== id) : [...prev.events, id],
      }
    })
  }

  async function save(e) {
    e.preventDefault()
    if (!canWrite) return
    setBusy(true)
    setOk('')
    try {
      const notify = {
        enabled: form.enabled,
        smtp_host: form.smtp_host,
        smtp_port: Number(form.smtp_port) || 587,
        smtp_tls: form.smtp_tls,
        smtp_user: form.smtp_user,
        from: form.from,
        recipients: form.recipients
          .split(/[\n,;]+/)
          .map((s) => s.trim())
          .filter(Boolean),
        events: form.events,
      }
      if (form.smtp_password) {
        notify.smtp_password = form.smtp_password
      }
      const next = await api('/v1/settings', {
        method: 'PUT',
        body: { node_name: form.node_name.trim(), notify },
      })
      setForm(fromApi(next))
      setOk('Settings saved.')
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function testMail() {
    if (!canWrite) return
    setBusy(true)
    setOk('')
    try {
      await api('/v1/settings/mail/test', { method: 'POST', body: {} })
      setOk('Test email sent.')
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="settings" size={20} />
            Settings
          </h1>
          <p className="dash-lead muted">
            Node identity and shared SMTP notifications for cluster events.
          </p>
        </div>
      </div>

      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}
      {ok && (
        <div className="banner success">
          {ok}
          <button type="button" className="banner-dismiss" onClick={() => setOk('')}>
            ×
          </button>
        </div>
      )}

      {!loaded ? (
        <p className="muted">Loading…</p>
      ) : (
        <form className="card" onSubmit={save}>
          <h2 className="card-title">Node</h2>
          <div className="form-grid">
            <label>
              Node name
              <input
                value={form.node_name}
                disabled={!canWrite || busy}
                onChange={(e) => setField('node_name', e.target.value)}
                required
              />
            </label>
          </div>

          <h2 className="card-title" style={{ marginTop: '1.25rem' }}>
            Mail notifications
          </h2>
          <label className="check-row">
            <input
              type="checkbox"
              checked={form.enabled}
              disabled={!canWrite || busy}
              onChange={(e) => setField('enabled', e.target.checked)}
            />
            Enable event emails
          </label>

          <div className="form-grid" style={{ marginTop: '0.75rem' }}>
            <label>
              SMTP host
              <input
                value={form.smtp_host}
                disabled={!canWrite || busy}
                onChange={(e) => setField('smtp_host', e.target.value)}
                placeholder="smtp.example.com"
              />
            </label>
            <label>
              Port
              <input
                type="number"
                min={1}
                max={65535}
                value={form.smtp_port}
                disabled={!canWrite || busy}
                onChange={(e) => setField('smtp_port', e.target.value)}
              />
            </label>
            <label>
              TLS
              <select
                value={form.smtp_tls}
                disabled={!canWrite || busy}
                onChange={(e) => setField('smtp_tls', e.target.value)}
              >
                <option value="off">Off</option>
                <option value="starttls">STARTTLS</option>
                <option value="tls">TLS</option>
              </select>
            </label>
            <label>
              SMTP user
              <input
                value={form.smtp_user}
                disabled={!canWrite || busy}
                onChange={(e) => setField('smtp_user', e.target.value)}
                autoComplete="off"
              />
            </label>
            <label>
              SMTP password
              <input
                type="password"
                value={form.smtp_password}
                disabled={!canWrite || busy}
                onChange={(e) => setField('smtp_password', e.target.value)}
                placeholder={form.smtp_password_set ? '(unchanged)' : ''}
                autoComplete="new-password"
              />
            </label>
            <label>
              From address
              <input
                type="email"
                value={form.from}
                disabled={!canWrite || busy}
                onChange={(e) => setField('from', e.target.value)}
                placeholder="pertisk@example.com"
              />
            </label>
            <label style={{ gridColumn: '1 / -1' }}>
              Recipients (shared)
              <textarea
                rows={3}
                value={form.recipients}
                disabled={!canWrite || busy}
                onChange={(e) => setField('recipients', e.target.value)}
                placeholder="ops@example.com"
              />
            </label>
          </div>

          <h3 style={{ marginTop: '1rem', fontSize: '0.95rem' }}>Events</h3>
          <div className="settings-events">
            {ALL_EVENTS.map((ev) => (
              <label key={ev.id} className="check-row">
                <input
                  type="checkbox"
                  checked={form.events.includes(ev.id)}
                  disabled={!canWrite || busy}
                  onChange={() => toggleEvent(ev.id)}
                />
                {ev.label}
                <span className="muted" style={{ marginLeft: '0.35rem' }}>
                  ({ev.id})
                </span>
              </label>
            ))}
          </div>

          {canWrite && (
            <div className="row-actions" style={{ marginTop: '1.25rem' }}>
              <Btn type="submit" icon="check" disabled={busy}>
                Save
              </Btn>
              <Btn
                type="button"
                icon="bell"
                variant="secondary"
                disabled={busy}
                onClick={testMail}
              >
                Test email
              </Btn>
            </div>
          )}
        </form>
      )}
    </div>
  )
}
