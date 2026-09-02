import { useEffect, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { api, asList } from '../api'
import { Btn, Icon } from '../components/Icons'
import Modal from '../components/Modal'
import ChangePassword from '../components/ChangePassword'
import { useConfirm } from '../components/Confirm'

export default function Users() {
  const { user } = useOutletContext()
  const confirm = useConfirm()
  const [users, setUsers] = useState([])
  const [error, setError] = useState('')
  const [open, setOpen] = useState(false)
  const [passwordUser, setPasswordUser] = useState(null)
  const [form, setForm] = useState({ username: '', password: '', role: 'operator' })
  const [busy, setBusy] = useState(false)

  async function refresh() {
    try {
      setUsers(asList(await api('/v1/users')))
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    }
  }

  useEffect(() => {
    refresh()
  }, [])

  async function createUser(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await api('/v1/users', { method: 'POST', body: form })
      setForm({ username: '', password: '', role: 'operator' })
      setOpen(false)
      await refresh()
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
            <Icon name="users" size={20} />
            Users
          </h1>
          <p className="dash-lead muted">Admin, operator, and viewer accounts on this control plane.</p>
        </div>
        <Btn icon="plus" onClick={() => setOpen(true)}>
          Add user
        </Btn>
      </div>
      {error && (
        <div className="banner danger">
          {error}
          <button type="button" className="banner-dismiss" onClick={() => setError('')}>
            ×
          </button>
        </div>
      )}
      <section className="card table-card">
        {users.length === 0 ? (
          <p className="muted">No users returned.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Username</th>
                  <th>Role</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.id}>
                    <td>{u.username}</td>
                    <td>
                      <span className="badge">{u.role}</span>
                    </td>
                    <td className="col-actions">
                      <div className="row-actions">
                        <Btn
                          icon="key"
                          variant="secondary"
                          onClick={() => setPasswordUser(u)}
                        >
                          Password
                        </Btn>
                        {u.id !== user?.id && (
                          <Btn
                            icon="trash"
                            variant="danger"
                            onClick={async () => {
                              const ok = await confirm({
                                title: 'Delete user',
                                message: `Remove ${u.username}?`,
                                confirmLabel: 'Delete',
                              })
                              if (ok) {
                                await api(`/v1/users/${u.id}`, { method: 'DELETE' })
                                refresh()
                              }
                            }}
                          >
                            Delete
                          </Btn>
                        )}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {passwordUser && (
        passwordUser.id === user?.id ? (
          <ChangePassword onClose={() => setPasswordUser(null)} />
        ) : (
          <ChangePassword
            userId={passwordUser.id}
            username={passwordUser.username}
            onClose={() => setPasswordUser(null)}
          />
        )
      )}

      {open && (
        <Modal
          title="Add user"
          onClose={() => setOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="create-user" disabled={busy}>
                Create
              </button>
            </>
          }
        >
          <form id="create-user" onSubmit={createUser}>
            <div className="field">
              <label htmlFor="user-name">Username</label>
              <input
                id="user-name"
                required
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="user-pass">Password</label>
              <input
                id="user-pass"
                type="password"
                required
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
              />
            </div>
            <p className="wizard-section-title">Role</p>
            <div className="role-pills">
              {['admin', 'operator', 'viewer'].map((role) => (
                <button
                  key={role}
                  type="button"
                  className={`role-pill${form.role === role ? ' active' : ''}`}
                  onClick={() => setForm({ ...form, role })}
                >
                  <strong>{role}</strong>
                  <span>
                    {role === 'admin' ? 'Full control' : role === 'operator' ? 'Mutate guests' : 'Read only'}
                  </span>
                </button>
              ))}
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
