import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from './api'
import { Artwork } from './Artwork'
import { presentRecord, recordData } from './discovery'
import { safeWebUrl } from './collectionEnrichment'
import { useIdentityMap } from './identities'
import { RetryNotice } from './DiscoveryCards'
import type { RepositoryRecord } from './CollectionsPage'

export function StoryPage({uri}:{uri:string}) {
  const query = useQuery({queryKey:['repository-record',uri],queryFn:()=>api<RepositoryRecord>(`/api/repository-record?uri=${encodeURIComponent(uri)}`)})
  const record = query.data
  const item = record && presentRecord(record)
  const data = record ? recordData(record) : {}
  const site = typeof data.site === 'string' && data.site.startsWith('at://') ? data.site : undefined
  const publication = useQuery({queryKey:['repository-record',site],queryFn:()=>api<RepositoryRecord>(`/api/repository-record?uri=${encodeURIComponent(site!)}`),enabled:!!site,staleTime:300_000})
  const handles = useIdentityMap(record ? [record.actor_did] : [])
  let source = item?.url
  let publicationName = ''
  if (publication.data && publication.data.operation !== 'delete') {
    const pub = recordData(publication.data)
    publicationName = typeof pub.name === 'string' ? pub.name : ''
    const base = safeWebUrl(pub.url)
    if (base && typeof data.path === 'string' && data.path.startsWith('/') && !data.path.startsWith('//')) source = safeWebUrl(new URL(data.path,base).href)
  }
  const body = [data.textContent,data.content,data.description].find(v=>typeof v === 'string') as string | undefined
  const hidden = data.isDraft === true || ['private','unlisted','author'].includes(String(data.visibility))
  return <article className="mx-auto max-w-3xl space-y-6">{query.isPending && <p role="status" className="panel p-8 text-dim">Opening this discovery…</p>}{query.isError && <RetryNotice retry={()=>void query.refetch()} text="We couldn’t open this item. It may no longer be available." />}{record && item && <>
    {record.operation === 'delete' || hidden ? <section className="panel p-8"><h1 className="text-2xl font-semibold">This item is no longer available.</h1><p className="mt-3 text-sm text-dim">It was removed or isn’t intended for public discovery.</p></section> : <>
      <header className="discovery-card p-6 sm:p-8"><p className="eyebrow">{item.action}{item.app ? ` · ${item.app.name}` : ''}</p>{item.artworkUrl && <div className="mt-6"><Artwork url={item.artworkUrl} className={item.topic === 'films' ? 'h-60 w-40' : 'size-40'} /></div>}<h1 className="mt-5 break-words text-3xl font-semibold leading-tight tracking-tight sm:text-4xl">{item.title}</h1><p className="mt-3 break-words text-sm leading-6 text-dim">{publicationName || item.subtitle}</p><Link to="/accounts/$did" params={{did:record.actor_did}} className="mt-5 inline-block text-sm text-cyan">{handles.get(record.actor_did) ? `By @${handles.get(record.actor_did)}` : 'Meet the person behind this'} →</Link>
      {item.date && Number.isFinite(Date.parse(item.date)) && <p className="mt-3 text-sm text-dim">{item.topic === 'events' ? 'Starts ' : ''}{new Date(item.date).toLocaleString([], {dateStyle:'long',timeStyle:item.topic === 'events' ? 'short' : undefined})}</p>}
      {item.topic === 'events' && typeof data.endsAt === 'string' && Number.isFinite(Date.parse(data.endsAt)) && <p className="mt-1 text-sm text-dim">Ends {new Date(data.endsAt).toLocaleString([], {dateStyle:'long',timeStyle:'short'})}</p>}
      {item.topic === 'events' && <p className="mt-3 text-xs text-dim">Dates and location are supplied by the organizer. Check the original listing before making plans.</p>}
      {item.topic === 'streams' && <p className="mt-3 text-xs text-dim">This is a retained announcement. The stream may have ended.</p>}
      {source && <a href={source} target="_blank" rel="noopener noreferrer" className="mt-6 inline-block rounded-full bg-cyan px-5 py-3 text-sm font-semibold text-ink">{item.topic === 'writing' ? 'Read original' : item.topic === 'events' ? 'Event details & RSVP' : item.topic === 'music' ? 'Listen at source' : item.topic === 'streams' ? 'Check the stream' : item.topic === 'films' ? 'About the film' : 'Open original'} ↗</a>}
      {!source && item.app && <a href={item.app.url} target="_blank" rel="noopener noreferrer" className="mt-6 inline-block text-sm text-cyan">Explore on {item.app.name} ↗</a>}
      </header>
      {body && <section className="px-2 sm:px-5"><h2 className="mb-4 text-sm font-medium text-dim">{item.topic === 'writing' ? 'A preview of the story' : 'More about this'}</h2><div className="space-y-4 text-base leading-8 text-white/85">{body.slice(0,3000).split(/\n\s*\n/).slice(0,15).map((p,i)=><p key={i} className="whitespace-pre-wrap break-words">{p}</p>)}</div>{body.length > 3000 && <p className="mt-4 text-sm text-dim">Preview shortened. Open the original for the full piece.</p>}</section>}
      <section className="panel flex flex-wrap items-center justify-between gap-4 p-5"><div><h2 className="font-medium">Keep exploring</h2><p className="mt-1 text-sm text-dim">There’s more beyond this one item.</p></div><Link to="/explore" search={{topic:item.topic || ''}} className="text-sm text-cyan">More {item.topic === 'writing' ? 'writing' : item.topic || 'discoveries'} →</Link>{item.app && <Link to="/apps/$appId" params={{appId:item.app.id}} className="text-sm text-cyan">About {item.app.name} →</Link>}</section>
    </>}
    <details className="text-xs leading-6 text-dim"><summary>Source & technical details</summary><p className="mt-3">Last observed {new Date(record.timestamp_ms).toLocaleString()}. This is retained public data, not a live check of its source.</p><Link to="/records/$uri" params={{uri}} className="text-cyan">Inspect the underlying record →</Link></details>
  </>}</article>
}
