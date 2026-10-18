import { useState } from 'react'
import { Disc3 } from 'lucide-react'
import { safeArtworkUrl } from './collectionEnrichment'

/** Fixed dimensions avoid layout shift; failure is scoped to the current URL. */
export function Artwork({ url, className = 'size-12' }: { url?: string | null; className?: string }) {
  const [failedUrl, setFailedUrl] = useState<string>()
  const src = safeArtworkUrl(url)
  return <span className={`flex shrink-0 items-center justify-center overflow-hidden rounded-lg border border-line bg-white/5 ${className}`}>
    {src && failedUrl !== src ? <img src={src} alt="" loading="lazy" decoding="async" referrerPolicy="no-referrer" className="h-full w-full object-cover" onError={() => setFailedUrl(src)} /> : <Disc3 aria-hidden="true" className="size-5 text-dim" />}
  </span>
}
