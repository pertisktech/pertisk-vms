import { formatBytes } from '../api'

function Meter({ label, used, total, unit = '', detail }) {
  const pct =
    unit === '%'
      ? Math.min(100, Math.round(Number(used) || 0))
      : total > 0
        ? Math.min(100, Math.round((used / total) * 100))
        : 0
  let right = `${pct}%`
  if (unit === '%') {
    right = `${pct}%`
  } else if (total > 0 && unit === 'B') {
    right = `${pct}% (${formatBytes(used)} of ${formatBytes(total)})`
  } else if (total > 0) {
    right = `${pct}% (${used}${unit} of ${total}${unit})`
  }
  return (
    <div className="pve-meter">
      <div className="pve-meter-head">
        <span>{label}</span>
        <span className="muted">{right}</span>
      </div>
      <div className="pve-meter-track">
        <div
          className={`pve-meter-fill${pct > 90 ? ' hot' : pct > 75 ? ' warm' : ''}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      {detail && <div className="pve-meter-detail muted">{detail}</div>}
    </div>
  )
}

function rateLabel(bps) {
  if (!bps) return '0 B/s'
  if (bps < 1024) return `${bps} B/s`
  return `${formatBytes(bps)}/s`
}

/** Live CPU / memory / disk / network meters from a ResourceSample. */
export default function MetricsPanel({ live, title = 'Live', empty }) {
  if (!live) {
    return (
      <section className="card">
        <div className="table-meta">{title}</div>
        <p className="muted" style={{ margin: 0 }}>
          {empty || 'No live metrics'}
        </p>
      </section>
    )
  }

  return (
    <section className="card">
      <div className="table-meta">{title}</div>
      <div className="pve-meters">
        <Meter label="CPU" used={Math.round(live.cpu_pct || 0)} total={100} unit="%" />
        <Meter
          label="Memory"
          used={Number(live.mem_used_bytes) || 0}
          total={Number(live.mem_total_bytes) || 0}
          unit="B"
        />
        <Meter
          label="Disk"
          used={Number(live.disk_used_bytes) || 0}
          total={Number(live.disk_total_bytes) || 0}
          unit="B"
        />
        <div className="pve-meter">
          <div className="pve-meter-head">
            <span>Network</span>
            <span className="muted">
              ↓ {rateLabel(Number(live.net_rx_bps) || 0)} · ↑ {rateLabel(Number(live.net_tx_bps) || 0)}
            </span>
          </div>
          <div className="pve-meter-track">
            <div className="pve-meter-fill" style={{ width: '0%' }} />
          </div>
        </div>
      </div>
    </section>
  )
}

/** Allocated capacity meters (vCPU / memory). */
export function AllocatedPanel({ vcpusUsed, vcpusTotal, memUsedMib, memTotalMib }) {
  return (
    <section className="card">
      <div className="table-meta">Allocated</div>
      <div className="pve-meters">
        <Meter label="vCPU" used={vcpusUsed || 0} total={vcpusTotal || 0} unit="" />
        <Meter label="Memory" used={memUsedMib || 0} total={memTotalMib || 0} unit=" MiB" />
      </div>
    </section>
  )
}
