import { Icon } from '../components/Icons'
import { useInventory } from '../useInventory'

export default function Activity() {
  const { tasks, audit, error, setError } = useInventory()

  return (
    <div className="dash-page">
      <div className="page-head">
        <div>
          <h1>
            <Icon name="activity" size={20} />
            Activity
          </h1>
          <p className="dash-lead muted">Tasks and audit trail from this control plane.</p>
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

      <section className="card table-card">
        <div className="table-meta">Tasks</div>
        {tasks.length === 0 ? (
          <p className="muted">No tasks yet.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Kind</th>
                  <th>Status</th>
                  <th>Actor</th>
                  <th>Target</th>
                </tr>
              </thead>
              <tbody>
                {tasks.map((t) => (
                  <tr key={t.id || `${t.kind}-${t.actor}-${t.target}`}>
                    <td>{t.kind}</td>
                    <td>
                      <span className={`badge ${t.status === 'ok' || t.status === 'succeeded' ? 'ready' : t.status}`}>
                        {t.status}
                      </span>
                    </td>
                    <td>{t.actor}</td>
                    <td className="mono-inline">{t.target || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="card table-card">
        <div className="table-meta">Audit</div>
        {audit.length === 0 ? (
          <p className="muted">No audit events.</p>
        ) : (
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Actor</th>
                  <th>Action</th>
                  <th>Target</th>
                </tr>
              </thead>
              <tbody>
                {audit.map((a, i) => (
                  <tr key={a.id || `${a.actor}-${a.action}-${i}`}>
                    <td>{a.actor}</td>
                    <td>{a.action}</td>
                    <td className="mono-inline">{a.target || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
