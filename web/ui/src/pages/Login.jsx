import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { api, getToken, setToken } from '../api'
import { Icon } from '../components/Icons'
import { useState } from 'react'

export default function Login() {
  const nav = useNavigate()
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(true)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [version, setVersion] = useState('')

  useEffect(() => {
    if (getToken()) nav('/')
    api('/v1/health')
      .then((h) => setVersion(h.version || ''))
      .catch(() => {})
  }, [nav])

  async function onSubmit(e) {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const res = await api('/v1/login', {
        method: 'POST',
        body: { username, password },
      })
      setToken(res.token, remember)
      nav('/')
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="login-wrap">
      <div className="login-card">
        <div className="login-brand">
          <span className="login-brand-mark" aria-hidden>
            <Icon name="guests" size={18} />
          </span>
          <h1>Pertisk VM</h1>
        </div>
        <p>Sign in to the virtualization control plane.</p>
        {version ? <p className="login-version">v{version}</p> : null}
        {error && <div className="error">{error}</div>}
        <form onSubmit={onSubmit}>
          <div className="field">
            <label htmlFor="login-username">Username</label>
            <input
              id="login-username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
            />
          </div>
          <div className="field">
            <label htmlFor="login-password">Password</label>
            <input
              id="login-password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
            />
          </div>
          <label className="chk login-remember">
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
            <span className="chk-box" />
            <span className="chk-label">Stay signed in</span>
          </label>
          <button type="submit" className="login-submit" disabled={loading}>
            {loading ? 'Signing in…' : 'Sign in'}
          </button>
        </form>
      </div>
    </div>
  )
}
