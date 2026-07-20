import { useEffect, useState } from 'react'
import QRCode from 'qrcode'

export function QR({ text, size = 180 }: { text: string; size?: number }) {
  const [url, setUrl] = useState('')
  useEffect(() => {
    QRCode.toDataURL(text, { margin: 1, width: size * 2, color: { dark: '#101018', light: '#ffffff' } })
      .then(setUrl).catch(() => setUrl(''))
  }, [text, size])
  if (!url) return <div style={{ width: size, height: size, background: '#fff', borderRadius: 14 }} />
  return <img src={url} width={size} height={size} style={{ borderRadius: 14, background: '#fff', padding: 10 }} alt="QR" />
}
