import { useOutletContext } from 'react-router-dom'
import { asList, formatBytes, isTemplate } from '../api'
import MetricCard from '../components/MetricCard'
import MetricsCharts from '../components/MetricsCharts'
import { useMetrics } from '../useMetrics'

function pctLabel(n) {
  if (n == null || !Number.isFinite(n)) return '—'
  return `${n >= 10 ? n.toFixed(0) : n.toFixed(1)}%`
}

function cpuTone(pct) {
  if (!Number.isFinite(pct)) return { hint: undefined, hintTone: undefined, barTone: undefined }
  if (pct >= 80) return { hint: 'High', hintTone: undefined, barTone: 'hot' }
  if (pct >= 60) return { hint: 'Elevated', hintTone: undefined, barTone: 'warm' }
  return { hint: 'Normal', hintTone: 'ok', barTone: undefined }
}

export default function Overview() {
  const { inv } = useOutletContext()
  const { cluster, vms, error } = inv
  const metrics = useMetrics('cluster')
  const members = asList(cluster?.members)
  const online = members.filter((m) => m.online).length || (members.length ? 0 : 1)
  const nodeCount = members.length || 1
  const guests = vms.filter((vm) => !isTemplate(vm))
  const running = guests.filter((vm) => vm.state === 'running').length

  const cpuPct = Number(metrics.data?.live?.cpu_pct)
  const memTotal = Number(metrics.data?.live?.mem_total_bytes)
  const memUsed = Number(metrics.data?.live?.mem_used_bytes)
  const memPct = memTotal > 0 ? (memUsed / memTotal) * 100 : null
  const cpu = cpuTone(cpuPct)

  return (
    <div className="pve-stack">
      {error && error !== 'unauthorized' && <div className="banner danger">{error}</div>}

      <div className="dash-stat-row">
        <MetricCard
          icon="cpu"
          label="CPU load"
          value={pctLabel(cpuPct)}
          hint={cpu.hint}
          hintTone={cpu.hintTone}
          pct={cpuPct}
          barTone={cpu.barTone}
        />
        <MetricCard
          icon="memory"
          label="Memory"
          value={pctLabel(memPct)}
          hint={memTotal > 0 ? `${formatBytes(memUsed)} / ${formatBytes(memTotal)}` : undefined}
          pct={memPct}
        />
        <MetricCard
          icon="guests"
          label="Virtual machines"
          value={`${running} / ${guests.length}`}
          hint={`${running} running`}
          hintTone="ok"
          pct={guests.length ? (running / guests.length) * 100 : 0}
          barTone="ok"
        />
        <MetricCard
          icon="worker"
          label="Nodes"
          value={`${online} / ${nodeCount}`}
          hint={cluster?.quorum ? 'Quorum held' : 'No quorum'}
          hintTone={cluster?.quorum ? 'ok' : undefined}
          pct={(online / nodeCount) * 100}
          barTone={cluster?.quorum ? 'ok' : 'hot'}
        />
      </div>

      <MetricsCharts
        scope="cluster"
        title="Cluster resources"
        history={metrics.history}
        latest={metrics.data}
        nodes={asList(metrics.data?.nodes)}
        members={members}
        host={inv.host}
        selfId={cluster?.self_id || inv.host?.node_id}
        live={metrics.live}
        setLive={metrics.setLive}
        loading={metrics.loading}
        onRefresh={() => metrics.refresh()}
      />
    </div>
  )
}
