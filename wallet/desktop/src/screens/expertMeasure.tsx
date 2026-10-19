import { useI18n } from '../i18n'
import type { ExpertMeasurementProgress } from '../api'

export type MeasureState = { running: boolean; error: string | null }
export async function runMeasurement({ benchmark, refresh, setState, failed, noTarget }: {
  benchmark: () => Promise<{ ok: boolean; error?: string; errorCode?: string }>
  refresh: () => Promise<unknown>; setState: (state: MeasureState) => void; failed: string; noTarget: string
}) {
  setState({ running: true, error: null })
  let error: string | null = null
  try {
    const result = await benchmark()
    if (!result.ok) error = result.errorCode === 'noMeasurementTarget' ? noTarget : result.error || failed
    await refresh()
  } catch (e) { error ||= e instanceof Error ? e.message : failed }
  finally { setState({ running: false, error }) }
}

export function ExpertMeasureControl({ state, progress, disabled, ready = true, onMeasure }: {
  state: MeasureState; progress?: ExpertMeasurementProgress | null; disabled?: boolean
  /** false until an expert shard is downloaded and a worker serves it */
  ready?: boolean; onMeasure: () => void
}) {
  const { t } = useI18n()
  const running = state.running || !!progress?.running
  return <div style={{ marginTop: 8 }}>
    <button className="btn block" disabled={disabled || running || !ready} aria-busy={running} onClick={onMeasure}>
      {running && <span className="measure-spinner" aria-hidden="true" />}
      {running ? t('ns.measuring') : t('ns.measure')}
    </button>
    {running && <div role="status" className="small muted">
      {progress?.running && progress.layer != null ? t('ns.expertMeasureProgress', progress.layer, progress.index, progress.count) : t('ns.measuring')}
      {progress?.running && <progress aria-label={t('ns.measuring')} value={Math.max(0, progress.index - 1)} max={progress.count} style={{ width: '100%', accentColor: 'var(--pink)' }} />}
    </div>}
    {!running && (state.error || progress?.error) && <div role="alert" className="small" style={{ color: 'var(--danger)' }}>{state.error || progress?.error}</div>}
    <div className="small muted">{t(ready ? 'ns.expertMeasureHint' : 'ns.expertMeasureWaiting')}</div>
  </div>
}
