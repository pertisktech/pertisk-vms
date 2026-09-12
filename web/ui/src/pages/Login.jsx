import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { api, clearAuthRequired, clearToken, getToken, setToken } from '../api'
import { Icon } from '../components/Icons'
import { useTheme } from '../ThemeContext'
import { useState } from 'react'

export default function Login() {
  const nav = useNavigate()
  const { appearance, toggleAppearance } = useTheme()
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(true)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [version, setVersion] = useState('')

  useEffect(() => {
    let cancelled = false
    api('/v1/health')
      .then((h) => {
        if (!cancelled) setVersion(h.version || '')
      })
      .catch(() => {})
    const token = getToken()
    if (!token) return () => {
      cancelled = true
    }
    api('/v1/session', { notifyAuth: false })
      .then(() => {
        if (!cancelled) nav('/')
      })
      .catch((err) => {
        if (err.status === 401) {
          clearToken()
          clearAuthRequired()
        }
      })
    return () => {
      cancelled = true
    }
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
      clearAuthRequired()
      nav('/')
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="login-wrap">
      <button
        type="button"
        className="pve-icon-btn login-theme"
        onClick={toggleAppearance}
        title={appearance === 'dark' ? 'Switch to light' : 'Switch to dark'}
        aria-label="Toggle color theme"
      >
        <Icon name={appearance === 'dark' ? 'sun' : 'moon'} size={16} />
      </button>
      <div className="login-card">
        <div className="login-brand">
          <span className="login-brand-mark" aria-hidden>
            <Icon name="worker" size={18} />
          </span>
          <div>
            <h1>
              Pertisk <span className="accent">VM</span>
            </h1>
            {version ? <p className="login-version">v{version}</p> : null}
          </div>
        </div>
        <p>Sign in to the virtualization control plane.</p>
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
              name="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              autoFocus
              spellCheck={false}
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
