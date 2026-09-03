import { Link } from 'react-router-dom'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { asList, formatBytes } from '../api'
import { Icon } from './Icons'

const CHART = {
  cpu: 'var(--color-primary)',
  mem: 'var(--color-blue-b1)',
  disk: 'var(--color-yellow-y1)',
  rx: 'var(--color-green-g1)',
  tx: 'var(--color-yellow-y1)',
}

const tooltipStyle = {
  backgroundColor: 'var(--color-surface-elevated)',
  border: '1px solid var(--color-border)',
  borderRadius: 8,
  color: 'var(--color-text)',
  fontSize: 12,
}

function formatRate(bps) {
  const n = Number(bps) || 0
  if (n < 1024) return `${n} B/s`
  return `${formatBytes(n)}/s`
}

function formatPct(value) {
  if (value == null || !Number.isFinite(value)) return '—'
  return `${value.toFixed(value >= 10 ? 0 : 1)}%`
}

function formatMibps(value) {
  if (value == null || !Number.isFinite(value)) return '—'
  return `${value.toFixed(2)} MiB/s`
}

function LegendDot({ color, label }) {
  return (
    <span className="metrics-legend-item">
      <span className="metrics-legend-dot" style={{ backgroundColor: color }} />
      {label}
    </span>
  )
}

function EmptyChart({ message }) {
  return <div className="metrics-chart-empty">{message}</div>
}

function Stat({ label, value }) {
  return (
    <div className="stat">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  )
}

function ChartCard({ title, hint, legend, children, empty }) {
  return (
    <section className="card metrics-chart-card">
      <div className="metrics-chart-head">
        <div>
          <h2>{title}</h2>
          {hint && <p className="metrics-chart-hint">{hint}</p>}
        </div>
        {legend && <div className="metrics-legend">{legend}</div>}
      </div>
      {empty ? <EmptyChart message={empty} /> : <div className="metrics-chart">{children}</div>}
    </section>
  )
}

const HINTS = {
  cluster: {
    cpu: 'Percent of host cores',
    mem: 'Used percent of installed RAM and storage root.',
    net: 'Receive and transmit, from successive host samples.',
  },
  node: {
    cpu: 'Percent of this node’s cores',
    mem: 'Used percent of this node’s RAM and storage root.',
    net: 'Receive and transmit on this node.',
  },
  vm: {
    cpu: 'Guest CPU as a share of host cores',
    mem: 'Guest RSS versus assigned memory, and disk image usage.',
    net: 'Receive and transmit on the guest TAP.',
  },
}

export default function MetricsCharts({
  history,
  latest,
  nodes,
  live,
  setLive,
  loading,
  onRefresh,
  title,
  extras,
  empty,
  scope = 'cluster',
}) {
  const current = history[history.length - 1]
  const sample = latest?.live || current
  const waiting = history.length === 0
  const emptyMsg =
    empty || (loading ? 'Loading…' : 'No time-series data yet — wait for the next sample.')
  const hints = HINTS[scope] || HINTS.cluster
  const gid = (name) => `${scope}-${name}`
  const nodeRows = asList(nodes)

  const nodeBars = nodeRows.map((n) => {
    const liveSample = n.live || {}
    const memPct =
      liveSample.mem_total_bytes > 0
        ? Math.round((liveSample.mem_used_bytes / liveSample.mem_total_bytes) * 1000) / 10
        : 0
    const diskPct =
      liveSample.disk_total_bytes > 0
        ? Math.round((liveSample.disk_used_bytes / liveSample.disk_total_bytes) * 1000) / 10
        : 0
    return {
      name: n.name || n.node_id,
      cpu: Math.round((liveSample.cpu_pct || 0) * 10) / 10,
      mem: memPct,
      disk: diskPct,
      running: n.running_vms ?? 0,
      node_id: n.node_id,
    }
  })

  return (
    <div className="metrics-board">
      <div className="metrics-toolbar">
        <label className="metrics-check">
          <input type="checkbox" checked={live} onChange={(e) => setLive(e.target.checked)} />
          Live refresh
        </label>
        <button type="button" className="metrics-refresh" onClick={onRefresh} disabled={loading}>
          <Icon name="refresh" size={14} className={loading ? 'spin' : ''} />
          Refresh
        </button>
        {title && <span className="muted">{title}</span>}
      </div>

      <div className="dash-stat-row metrics-stat-row">
        <Stat label="CPU" value={formatPct(sample?.cpu ?? sample?.cpu_pct)} />
        <Stat
          label="Memory"
          value={
            sample?.mem_total || sample?.mem_total_bytes
              ? `${formatBytes(sample.mem_used ?? sample.mem_used_bytes)} / ${formatBytes(
                  sample.mem_total ?? sample.mem_total_bytes,
                )}`
              : '—'
          }
        />
        <Stat
          label="Disk"
          value={
            sample?.disk_total || sample?.disk_total_bytes
              ? `${formatBytes(sample.disk_used ?? sample.disk_used_bytes)} / ${formatBytes(
                  sample.disk_total ?? sample.disk_total_bytes,
                )}`
              : '—'
          }
        />
        <Stat
          label="Network"
          value={`↓ ${formatRate(sample?.rx ?? sample?.net_rx_bps)} · ↑ ${formatRate(
            sample?.tx ?? sample?.net_tx_bps,
          )}`}
        />
        {extras}
      </div>

      <ChartCard
        title="CPU"
        hint={`${hints.cpu}${live ? ' (live every 3s)' : ''}.`}
        empty={waiting ? emptyMsg : null}
      >
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={history} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
            <defs>
              <linearGradient id={gid('cpuFill')} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART.cpu} stopOpacity={0.35} />
                <stop offset="100%" stopColor={CHART.cpu} stopOpacity={0} />
              </linearGradient>
            </defs>
            <CartesianGrid stroke="var(--color-border)" strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="time" tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} />
            <YAxis
              domain={[0, 100]}
              tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }}
              width={40}
              tickFormatter={(v) => `${v}%`}
            />
            <Tooltip
              contentStyle={tooltipStyle}
              labelStyle={{ color: 'var(--color-text-secondary)' }}
              formatter={(value) => [formatPct(Number(value)), 'CPU']}
            />
            <Area
              type="monotone"
              dataKey="cpu"
              stroke={CHART.cpu}
              fill={`url(#${gid('cpuFill')})`}
              strokeWidth={2}
              isAnimationActive={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </ChartCard>

      <div className="metrics-chart-grid">
        <ChartCard
          title="Memory & disk"
          hint={hints.mem}
          legend={
            <>
              <LegendDot color={CHART.mem} label="Memory" />
              <LegendDot color={CHART.disk} label="Disk" />
            </>
          }
          empty={waiting ? emptyMsg : null}
        >
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={history} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
              <defs>
                <linearGradient id={gid('memFill')} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={CHART.mem} stopOpacity={0.35} />
                  <stop offset="100%" stopColor={CHART.mem} stopOpacity={0} />
                </linearGradient>
                <linearGradient id={gid('diskFill')} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={CHART.disk} stopOpacity={0.3} />
                  <stop offset="100%" stopColor={CHART.disk} stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid stroke="var(--color-border)" strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="time" tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} />
              <YAxis
                domain={[0, 100]}
                tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }}
                width={40}
                tickFormatter={(v) => `${v}%`}
              />
              <Tooltip
                contentStyle={tooltipStyle}
                labelStyle={{ color: 'var(--color-text-secondary)' }}
                formatter={(value, name) => [formatPct(Number(value)), name === 'mem_pct' ? 'Memory' : 'Disk']}
              />
              <Area
                type="monotone"
                dataKey="mem_pct"
                stroke={CHART.mem}
                fill={`url(#${gid('memFill')})`}
                strokeWidth={2}
                isAnimationActive={false}
              />
              <Area
                type="monotone"
                dataKey="disk_pct"
                stroke={CHART.disk}
                fill={`url(#${gid('diskFill')})`}
                strokeWidth={2}
                isAnimationActive={false}
              />
            </AreaChart>
          </ResponsiveContainer>
        </ChartCard>

        <ChartCard
          title="Network throughput"
          hint={hints.net}
          legend={
            <>
              <LegendDot color={CHART.rx} label="Receive" />
              <LegendDot color={CHART.tx} label="Transmit" />
            </>
          }
          empty={waiting ? emptyMsg : null}
        >
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={history} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
              <defs>
                <linearGradient id={gid('rxFill')} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={CHART.rx} stopOpacity={0.35} />
                  <stop offset="100%" stopColor={CHART.rx} stopOpacity={0} />
                </linearGradient>
                <linearGradient id={gid('txFill')} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={CHART.tx} stopOpacity={0.3} />
                  <stop offset="100%" stopColor={CHART.tx} stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid stroke="var(--color-border)" strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="time" tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} />
              <YAxis tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} width={48} />
              <Tooltip
                contentStyle={tooltipStyle}
                labelStyle={{ color: 'var(--color-text-secondary)' }}
                formatter={(value, name) => [
                  formatMibps(Number(value)),
                  name === 'rx_mibps' ? 'Receive' : 'Transmit',
                ]}
              />
              <Area
                type="monotone"
                dataKey="rx_mibps"
                stroke={CHART.rx}
                fill={`url(#${gid('rxFill')})`}
                strokeWidth={2}
                isAnimationActive={false}
              />
              <Area
                type="monotone"
                dataKey="tx_mibps"
                stroke={CHART.tx}
                fill={`url(#${gid('txFill')})`}
                strokeWidth={2}
                isAnimationActive={false}
              />
            </AreaChart>
          </ResponsiveContainer>
        </ChartCard>
      </div>

      {nodeBars.length > 0 && (
        <ChartCard
          title="Nodes"
          hint="Latest CPU, memory, and disk percent per node."
          legend={
            <>
              <LegendDot color={CHART.cpu} label="CPU" />
              <LegendDot color={CHART.mem} label="Memory" />
              <LegendDot color={CHART.disk} label="Disk" />
            </>
          }
        >
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={nodeBars} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
              <CartesianGrid stroke="var(--color-border)" strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="name" tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }} />
              <YAxis
                domain={[0, 100]}
                tick={{ fill: 'var(--color-text-secondary)', fontSize: 11 }}
                width={40}
                tickFormatter={(v) => `${v}%`}
              />
              <Tooltip
                contentStyle={tooltipStyle}
                labelStyle={{ color: 'var(--color-text-secondary)' }}
                cursor={{ fill: 'var(--color-hover)', opacity: 0.4 }}
                formatter={(value, name) => [
                  formatPct(Number(value)),
                  name === 'cpu' ? 'CPU' : name === 'mem' ? 'Memory' : 'Disk',
                ]}
              />
              <Bar dataKey="cpu" fill={CHART.cpu} radius={[4, 4, 0, 0]} isAnimationActive={false}>
                {nodeBars.map((row) => (
                  <Cell key={`${row.node_id}-cpu`} fill={CHART.cpu} />
                ))}
              </Bar>
              <Bar dataKey="mem" fill={CHART.mem} radius={[4, 4, 0, 0]} isAnimationActive={false} />
              <Bar dataKey="disk" fill={CHART.disk} radius={[4, 4, 0, 0]} isAnimationActive={false} />
            </BarChart>
          </ResponsiveContainer>
        </ChartCard>
      )}

      {nodeRows.length > 0 && (
        <section className="card table-card">
          <div className="table-meta">Nodes</div>
          <div className="table-shell">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>CPU</th>
                  <th>Memory</th>
                  <th>Disk</th>
                  <th>Running</th>
                </tr>
              </thead>
              <tbody>
                {nodeRows.map((n) => {
                  const liveSample = n.live || {}
                  const memPct =
                    liveSample.mem_total_bytes > 0
                      ? Math.round((liveSample.mem_used_bytes / liveSample.mem_total_bytes) * 100)
                      : 0
                  const diskPct =
                    liveSample.disk_total_bytes > 0
                      ? Math.round((liveSample.disk_used_bytes / liveSample.disk_total_bytes) * 100)
                      : 0
                  return (
                    <tr key={n.node_id}>
                      <td>
                        <Link to={`/node/${n.node_id}/summary`} className="pve-link">
                          {n.name}
                        </Link>
                      </td>
                      <td>{Math.round(liveSample.cpu_pct || 0)}%</td>
                      <td>{memPct}%</td>
                      <td>{diskPct}%</td>
                      <td>{n.running_vms ?? 0}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </div>
  )
}
