import { useCallback, useEffect, useState } from 'react'
import { api, asList } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import Modal from '../../components/Modal'
import { useNode } from '../NodeView'

const EMPTY = {
  name: 'debian-security',
  uri: 'http://security.debian.org/debian-security',
  suite: 'trixie-security',
  components: 'main contrib non-free-firmware',
}

export default function NodeRepositories() {
  const { canWrite } = useNode()
  const [repos, setRepos] = useState([])
  const [error, setError] = useState('')
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setRepos(asList(await api('/v1/repositories')))
      setError('')
    } catch (err) {
      setError(err.message || String(err))
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  async function toggle(repo, enabled) {
    setBusy(true)
    try {
      await api('/v1/repositories', { method: 'PATCH', body: { id: repo.id, enabled } })
      await load()
    } catch (err) {
      setError(err.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  async function addRepo(e) {
    e.preventDefault()
    setBusy(true)
    try {
      await api('/v1/repositories', {
        method: 'POST',
        body: {
          name: form.name.trim(),
          uri: form.uri.trim(),
          suite: form.suite.trim(),
          components: form.components.trim() || 'main',
        },
      })
      setOpen(false)
      setForm(EMPTY)
      await load()
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
            <Icon name="repo" size={20} />
            Repositories
          </h1>
          <p className="dash-lead muted">Apt sources used when you Refresh / Upgrade this node.</p>
        </div>
        {canWrite && (
          <Btn icon="plus" onClick={() => setOpen(true)}>
            Add
          </Btn>
        )}
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
        {repos.length === 0 ? (
          <p className="muted">No apt repositories found on this node.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Enabled</th>
                  <th>Type</th>
                  <th>URI</th>
                  <th>Suite</th>
                  <th>Components</th>
                </tr>
              </thead>
              <tbody>
                {repos.map((repo) => (
                  <tr key={repo.id}>
                    <td>
                      {canWrite ? (
                        <label className="repo-enable">
                          <input
                            type="checkbox"
                            checked={Boolean(repo.enabled)}
                            disabled={busy}
                            onChange={(e) => toggle(repo, e.target.checked)}
                          />
                          <span>{repo.enabled ? 'yes' : 'no'}</span>
                        </label>
                      ) : repo.enabled ? (
                        'yes'
                      ) : (
                        'no'
                      )}
                    </td>
                    <td>{repo.type || 'deb'}</td>
                    <td className="mono-inline">{repo.uri}</td>
                    <td>{repo.suite || '—'}</td>
                    <td className="muted">{repo.components || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {open && (
        <Modal
          title="Add repository"
          onClose={() => setOpen(false)}
          footer={
            <>
              <button type="button" className="secondary" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button type="submit" form="add-repo" disabled={busy}>
                Add
              </button>
            </>
          }
        >
          <form id="add-repo" onSubmit={addRepo}>
            <div className="field">
              <label htmlFor="repo-name">Name</label>
              <input
                id="repo-name"
                required
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="repo-uri">URI</label>
              <input
                id="repo-uri"
                required
                value={form.uri}
                onChange={(e) => setForm({ ...form, uri: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="repo-suite">Suite</label>
              <input
                id="repo-suite"
                required
                value={form.suite}
                onChange={(e) => setForm({ ...form, suite: e.target.value })}
              />
            </div>
            <div className="field">
              <label htmlFor="repo-components">Components</label>
              <input
                id="repo-components"
                value={form.components}
                onChange={(e) => setForm({ ...form, components: e.target.value })}
              />
            </div>
          </form>
        </Modal>
      )}
    </div>
  )
}
