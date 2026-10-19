import { useEffect, useState } from 'react'
import { Card } from '../components'
import { useI18n } from '../i18n'
import { api, isElectron } from '../api'
import type { NetworkPlanStatus, NodeStatus } from '../api'
import { runMeasurement } from './expertMeasure'
import type { MeasureState } from './expertMeasure'
import { MachinePerfView } from './machinePerfView'

/** Nodes-page card: reads the same main-process state the node settings use. */
export function MachinePerfCard({ onOpenSettings }: { onOpenSettings?: () => void }) {
  const { t } = useI18n()
  const [status, setStatus] = useState<NodeStatus | null>(null)
  const [plan, setPlan] = useState<NetworkPlanStatus | null>(null)
  const [state, setState] = useState<MeasureState>({ running: false, error: null })
  const [progress, setProgress] = useState<any>(null)
  useEffect(() => {
    if (!isElectron || !api.node) return
    let alive = true
    const poll = () => {
      api.node!.status().then((s) => { if (alive) setStatus(s) }).catch(() => {})
      api.node!.networkPlan?.().then((p) => { if (alive) setPlan(p) }).catch(() => {})
    }
    poll()
    const id = setInterval(poll, 4000)
    return () => { alive = false; clearInterval(id) }
  }, [])
  useEffect(() => {
    if (!state.running || !api.node?.benchmarkProgress) return
    let alive = true
    const id = setInterval(() => { api.node!.benchmarkProgress!().then((p) => { if (alive) setProgress(p) }).catch(() => {}) }, 150)
    return () => { alive = false; clearInterval(id) }
  }, [state.running])
  if (!isElectron) return <Card><div className="label">{t('node.perfTitle')}</div><div className="small muted">{t('dev.getAppHint')}</div></Card>
  const measure = () => runMeasurement({
    benchmark: () => api.node!.benchmark(64),
    refresh: async () => { setStatus(await api.node!.status()) }, setState,
    failed: t('ns.measureFailed'), noTarget: t('ns.noMeasurementTarget'),
  })
  const ready = !!status?.expertWork?.slots?.some((s) => s.phase === 'serving')
  return <MachinePerfView status={status} plan={plan} onOpenSettings={onOpenSettings}
    measure={{ state, progress: state.running ? progress : null, ready }} onMeasure={() => void measure()} />
}
