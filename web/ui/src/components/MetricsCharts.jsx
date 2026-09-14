import { Link } from 'react-router-dom'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { asList, formatUptime } from '../api'
import { Icon } from './Icons'

const CHART = {
  cpu: 'var(--chart-1)',
  mem: 'var(--chart-3)',
  disk: 'var(--chart-2)',
  rx: 'var(--chart-2)',
  tx: 'var(--chart-5)',
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

function ChartCard({ title, legend, children, empty }) {
  return (
    <section className="card metrics-chart-card">
      <div className="metrics-chart-head">
        <h2>{title}</h2>
        {legend && <div className="metrics-legend">{legend}</div>}
      </div>
      {empty ? <EmptyChart message={empty} /> : <div className="metrics-chart">{children}</div>}
    </section>
  )
}

function ChartTooltip({ active, payload, label }) {
  if (!active || !payload?.length) return null
  return (
    <div className="metrics-chart-tooltip">
      <div className="metrics-chart-tooltip-time">{label}</div>
      {payload.map((item) => (
        <div key={item.dataKey} className="metrics-chart-tooltip-row">
          <span className="metrics-legend-dot" style={{ backgroundColor: item.stroke }} />
          <span>{item.name}</span>
          <span className="metrics-chart-tooltip-val">
            {item.unit === '%' ? formatPct(Number(item.value)) : formatMibps(Number(item.value))}
          </span>
        </div>
      ))}
    </div>
  )
}

function UsageChart({ data, metrics, domain }) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={data} margin={{ top: 4, right: 12, bottom: 0, left: -8 }}>
        <defs>
          {metrics.map((metric) => (
            <linearGradient key={metric.key} id={metric.gradient} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={metric.color} stopOpacity={0.35} />
              <stop offset="100%" stopColor={metric.color} stopOpacity={0.02} />
            </linearGradient>
          ))}
        </defs>
        <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
        <XAxis
          dataKey="time"
          tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }}
          tickLine={false}
          axisLine={{ stroke: 'var(--border)' }}
          minTickGap={48}
        />
        <YAxis
          domain={domain}
          tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }}
          tickLine={false}
          axisLine={false}
          width={44}
          tickFormatter={domain?.[1] === 100 ? (v) => `${v}` : undefined}
        />
        <Tooltip content={<ChartTooltip />} />
        {metrics.map((metric) => (
          <Area
            key={metric.key}
            type="monotone"
            dataKey={metric.key}
            name={metric.label}
            unit={metric.unit}
            stroke={metric.color}
            strokeWidth={2}
            fill={`url(#${metric.gradient})`}
            isAnimationActive={false}
            dot={false}
          />
        ))}
      </AreaChart>
    </ResponsiveContainer>
  )
}

function samplePct(live, usedKey, totalKey) {
  const total = Number(live?.[totalKey]) || 0
  if (total <= 0) return 0
  return Math.round(((Number(live?.[usedKey]) || 0) / total) * 1000) / 10
}

export default function MetricsCharts({
  history,
  nodes,
  members,
  host,
  selfId,
  live,
  setLive,
  loading,
  onRefresh,
  title,
  empty,
  scope = 'cluster',
}) {
  const waiting = history.length === 0
  const emptyMsg =
    empty || (loading ? 'Loading…' : 'No time-series data yet — wait for the next sample.')
  const gid = (name) => `${scope}-${name}`
  const metricRows = asList(nodes)
  const memberRows = asList(members).map((m) => {
    const metric = metricRows.find((n) => String(n.node_id) === String(m.id))
    if (metric) {
      return { ...metric, name: metric.name || m.name }
    }
    const memTotal = (Number(m.memory_mib) || 0) * 1024 * 1024
    const memUsed = (Number(m.used_memory_mib) || 0) * 1024 * 1024
    const cores = Number(m.cpus) || 0
    return {
      node_id: m.id,
      name: m.name,
      live: {
        cpu_pct: cores ? ((Number(m.used_vcpus) || 0) / cores) * 100 : 0,
        mem_used_bytes: memUsed,
        mem_total_bytes: memTotal,
        disk_used_bytes: 0,
        disk_total_bytes: 0,
      },
      running_vms: 0,
    }
  })
  const nodeRows = memberRows.length ? memberRows : metricRows
  const memberById = new Map(asList(members).map((m) => [String(m.id), m]))

  const cpuMemMetrics = [
    { key: 'cpu', label: 'CPU', color: CHART.cpu, unit: '%', gradient: gid('cpuFill') },
    { key: 'mem_pct', label: 'Memory', color: CHART.mem, unit: '%', gradient: gid('memFill') },
  ]
  const netMetrics = [
    { key: 'rx_mibps', label: 'Net In', color: CHART.rx, unit: ' MiB/s', gradient: gid('rxFill') },
    { key: 'tx_mibps', label: 'Net Out', color: CHART.tx, unit: ' MiB/s', gradient: gid('txFill') },
  ]

  const nodeBars = nodeRows.map((n) => {
    const liveSample = n.live || {}
    return {
      name: n.name || n.node_id,
      cpu: Math.round((liveSample.cpu_pct || 0) * 10) / 10,
      mem: samplePct(liveSample, 'mem_used_bytes', 'mem_total_bytes'),
      disk: samplePct(liveSample, 'disk_used_bytes', 'disk_total_bytes'),
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

      <div className="metrics-chart-grid">
        <ChartCard
          title="CPU / Memory usage"
          legend={
            <>
              <LegendDot color={CHART.cpu} label="CPU" />
              <LegendDot color={CHART.mem} label="Memory" />
            </>
          }
          empty={waiting ? emptyMsg : null}
        >
          <UsageChart data={history} metrics={cpuMemMetrics} domain={[0, 100]} />
        </ChartCard>
        <ChartCard
          title="Network throughput"
          legend={
            <>
              <LegendDot color={CHART.rx} label="Net In" />
              <LegendDot color={CHART.tx} label="Net Out" />
            </>
          }
          empty={waiting ? emptyMsg : null}
        >
          <UsageChart data={history} metrics={netMetrics} domain={[0, 'auto']} />
        </ChartCard>
      </div>

      {nodeBars.length > 0 && (
        <ChartCard
          title="Nodes"
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
              <CartesianGrid stroke="var(--border)" strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="name" tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} />
              <YAxis
                domain={[0, 100]}
                tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }}
                width={40}
                tickFormatter={(v) => `${v}%`}
              />
              <Tooltip
                contentStyle={{
                  backgroundColor: 'var(--color-card)',
                  border: '1px solid var(--border)',
                  borderRadius: 8,
                  color: 'var(--text)',
                  fontSize: 12,
                }}
                labelStyle={{ color: 'var(--text-muted)' }}
                cursor={{ fill: 'var(--bg-hover)', opacity: 0.4 }}
                formatter={(value, name) => [
                  formatPct(Number(value)),
                  name === 'cpu' ? 'CPU' : name === 'mem' ? 'Memory' : 'Disk',
                ]}
              />
              <Bar dataKey="cpu" fill={CHART.cpu} radius={[3, 3, 0, 0]} maxBarSize={34} isAnimationActive={false} />
              <Bar dataKey="mem" fill={CHART.mem} radius={[3, 3, 0, 0]} maxBarSize={34} isAnimationActive={false} />
              <Bar dataKey="disk" fill={CHART.disk} radius={[3, 3, 0, 0]} maxBarSize={34} isAnimationActive={false} />
            </BarChart>
          </ResponsiveContainer>
        </ChartCard>
      )}

      {nodeRows.length > 0 && (
        <section className="card table-card pve-nodes-panel">
          <div className="table-meta">Nodes</div>
          <div className="table-shell">
            <table className="pve-dense-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Status</th>
                  <th>CPU</th>
                  <th>Memory</th>
                  <th>Disk</th>
                  <th>Running</th>
                  <th>Uptime</th>
                </tr>
              </thead>
              <tbody>
                {nodeRows.map((n) => {
                  const liveSample = n.live || {}
                  const memPct = Math.round(samplePct(liveSample, 'mem_used_bytes', 'mem_total_bytes'))
                  const diskPct = Math.round(samplePct(liveSample, 'disk_used_bytes', 'disk_total_bytes'))
                  const member = memberById.get(String(n.node_id))
                  const online = member ? member.online !== false : true
                  const status = online ? 'online' : 'offline'
                  const isSelf =
                    selfId != null
                      ? String(n.node_id) === String(selfId)
                      : host && String(n.node_id) === String(host.node_id)
                  const uptime = isSelf && host?.uptime_secs ? formatUptime(host.uptime_secs) : '—'
                  return (
                    <tr key={n.node_id}>
                      <td>
                        <Link to={`/node/${n.node_id}/summary`} className="pve-link">
                          <Icon name="worker" size={14} />
                          {n.name}
                        </Link>
                      </td>
                      <td>
                        <span className={`badge ${status}`}>{status}</span>
                      </td>
                      <td className="mono-inline">{Math.round(liveSample.cpu_pct || 0)}%</td>
                      <td className="mono-inline">{memPct}%</td>
                      <td className="mono-inline">{diskPct}%</td>
                      <td className="mono-inline">{n.running_vms ?? 0}</td>
                      <td className="mono-inline muted">{uptime}</td>
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
