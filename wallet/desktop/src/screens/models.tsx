import { useEffect, useState } from 'react'
import { useI18n } from '../i18n'
import { Card } from '../components'
import { LocalModelInfo } from '../api'

function sizeLabel(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(0)} MB`
  return `${(bytes / (1 << 10)).toFixed(0)} KB`
}
const displayName = (name: string) => name.replace(/\.gguf$/i, '')

/// Manage models the hub has downloaded to this desktop node: list, see size,
/// delete. Downloaded models also appear in AI 추론's picker for on-device runs.
export function ModelsScreen() {
  const { t } = useI18n()
  const models = window.linkcpp?.models
  const [items, setItems] = useState<LocalModelInfo[]>([])
  const [dir, setDir] = useState('')
  const [pending, setPending] = useState<string | null>(null)

  const load = () => { models?.list().then(setItems).catch(() => setItems([])) }
  useEffect(() => { load(); models?.dir().then(setDir).catch(() => {}) }, [])

  if (!models) {
    return (
      <div className="screen">
        <h2>{t('models.title')}</h2>
        <Card><p className="muted">{t('models.desktopOnly')}</p></Card>
      </div>
    )
  }

  return (
    <div className="screen">
      <h2>{t('models.title')}</h2>
      <Card>
        <div className="models-head">
          <div>
            <div className="models-title">{t('models.storedTitle')}</div>
            <div className="muted small">{t('models.storedDesc')}</div>
          </div>
          <button className="btn ghost" onClick={load}>↻</button>
        </div>
        {dir && <div className="muted small mono" style={{ marginTop: 8 }}>{dir}</div>}
      </Card>

      {items.length === 0 ? (
        <Card><p className="muted">{t('models.emptyHint')}</p></Card>
      ) : (
        <div className="models-list">
          {items.map((m) => (
            <Card key={m.name} className="model-row">
              <div className="model-info">
                <span className="model-icon">🧠</span>
                <div>
                  <div className="model-name">{displayName(m.name)}</div>
                  <div className="muted small">{sizeLabel(m.sizeBytes)}</div>
                </div>
              </div>
              {pending === m.name ? (
                <div className="model-confirm">
                  <span className="muted small">{t('models.deleteConfirm')}</span>
                  <button className="btn danger sm" onClick={() => { models.remove(m.name).then(setItems); setPending(null) }}>
                    {t('models.delete')}
                  </button>
                  <button className="btn ghost sm" onClick={() => setPending(null)}>{t('common.cancel')}</button>
                </div>
              ) : (
                <button className="btn ghost sm" onClick={() => setPending(m.name)}>🗑</button>
              )}
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
