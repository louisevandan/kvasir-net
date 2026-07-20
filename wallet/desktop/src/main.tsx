import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './theme.css'
import App from './App'
import { WalletProvider } from './state'
import { I18nProvider, Lang } from './i18n'
import { api } from './api'

async function boot() {
  // theme
  const theme = localStorage.getItem('theme') || 'dark'
  document.documentElement.dataset.theme = theme
  // language (persisted in main config)
  let initialLang: Lang | null = null
  try { initialLang = (await api.config.get()).language as Lang | null } catch {}

  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <I18nProvider initial={initialLang} onChange={(l) => api.config.set({ language: l })}>
        <WalletProvider>
          <App />
        </WalletProvider>
      </I18nProvider>
    </StrictMode>,
  )
}
boot()
