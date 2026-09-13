import { Icon } from './Icons'

function toneClass(pct) {
  if (pct == null || !Number.isFinite(pct)) return ''
  if (pct >= 90) return 'hot'
  if (pct >= 75) return 'warm'
  return ''
}

export default function MetricCard({ icon, label, value, hint, hintTone, pct, barTone }) {
  const n = Number(pct)
  const showBar = Number.isFinite(n)
  const fill = barTone || toneClass(n)
  return (
    <div className="metric-card metric-card-stack">
      <div className="metric-card-top">
        <div className="metric-card-title">
          {icon ? (
            <span className="metric-card-icon">
              <Icon name={icon} size={16} />
            </span>
          ) : null}
          <span className="metric-card-label">{label}</span>
        </div>
        {hint != null && hint !== '' && (
          <span className={`metric-card-hint${hintTone === 'ok' ? ' ok' : ''}`}>{hint}</span>
        )}
      </div>
      <div className="metric-card-value">{value}</div>
      {showBar && (
        <div className="metric-card-track">
          <div
            className={`metric-card-fill${fill ? ` ${fill}` : ''}`}
            style={{ width: `${Math.max(0, Math.min(100, n))}%` }}
          />
        </div>
      )}
    </div>
  )
}
