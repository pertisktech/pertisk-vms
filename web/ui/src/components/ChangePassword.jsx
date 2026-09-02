import { useState } from 'react'
import { api } from '../api'
import Modal from './Modal'

export default function ChangePassword({ onClose, userId, username }) {
  const self = !userId
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(e) {
    e.preventDefault()
    if (newPassword.length < 4) {
      setError('New password must be at least 4 characters.')
      return
    }
    if (newPassword !== confirm) {
      setError('New passwords do not match.')
      return
    }
    setBusy(true)
    setError('')
    try {
      if (self) {
        await api('/v1/session/password', {
          method: 'POST',
          body: { current_password: currentPassword, new_password: newPassword },
        })
      } else {
        await api(`/v1/users/${userId}/password`, {
          method: 'POST',
          body: { new_password: newPassword },
        })
      }
      onClose()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      title={self ? 'Change password' : `Set password · ${username || 'user'}`}
      hint={
        self
          ? 'Enter your current password, then a new one. Other sessions on this account will be signed out.'
          : 'This replaces the password for this account. Their other sessions will be signed out.'
      }
      onClose={onClose}
      footer={
        <>
          <button type="button" className="secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" form="change-password" disabled={busy}>
            {busy ? 'Saving…' : 'Save password'}
          </button>
        </>
      }
    >
      <form id="change-password" onSubmit={submit}>
        {error && <p className="banner danger">{error}</p>}
        {self && (
          <div className="field">
            <label htmlFor="pw-current">Current password</label>
            <input
              id="pw-current"
              type="password"
              autoComplete="current-password"
              required
              value={currentPassword}
              onChange={(e) => setCurrentPassword(e.target.value)}
            />
          </div>
        )}
        <div className="field">
          <label htmlFor="pw-new">New password</label>
          <input
            id="pw-new"
            type="password"
            autoComplete="new-password"
            required
            minLength={4}
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor="pw-confirm">Confirm new password</label>
          <input
            id="pw-confirm"
            type="password"
            autoComplete="new-password"
            required
            minLength={4}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
          />
        </div>
      </form>
    </Modal>
  )
}
