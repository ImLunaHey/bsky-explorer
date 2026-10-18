import { Link } from '@tanstack/react-router'
import { ArrowUpRight } from 'lucide-react'
import { Artwork } from './Artwork'
import { presentRecord } from './discovery'
import { useIdentityMap } from './identities'
import type { RepositoryRecord } from './CollectionsPage'
import type { Ranking } from './discovery'

export function ContentCards({ records }: { records: RepositoryRecord[] }) {
  const handles = useIdentityMap(records.map(record => record.actor_did))
  return <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">{records.map(record => <ContentCard key={record.at_uri} record={record} handle={handles.get(record.actor_did)} />)}</div>
}
function ContentCard({ record, handle }: { record: RepositoryRecord; handle?: string }) {
  const item = presentRecord(record)
  return <article className="discovery-card flex min-w-0 flex-col p-5">
    <p className="eyebrow">{item.action}{item.app ? ` · ${item.app.name}` : ''}</p>
    {item.artworkUrl && <div className="mt-4"><Artwork url={item.artworkUrl} className={item.topic === 'films' ? 'h-36 w-24' : 'size-24'} /></div>}
    <Link to="/stories/$uri" params={{uri:record.at_uri}} className="mt-4 block break-words text-xl font-semibold leading-snug tracking-tight hover:text-cyan">{item.title}</Link>
    {item.topic === 'events' && item.date && Number.isFinite(Date.parse(item.date)) && <p className="mt-3 text-sm text-cyan">{new Date(item.date).toLocaleString([], {dateStyle:'medium',timeStyle:'short'})}</p>}
    <p className="mt-2 line-clamp-2 break-words text-xs text-dim">{item.subtitle}</p>
    {item.body && <p className="mt-3 line-clamp-3 break-words text-sm leading-6 text-dim">{item.body}</p>}
    <div className="mt-auto flex items-end justify-between gap-3 pt-5 text-xs"><Link to="/accounts/$did" params={{did:record.actor_did}} className="min-w-0 truncate text-cyan">{handle ? `@${handle}` : 'Meet the person →'}</Link><Link to="/stories/$uri" params={{uri:record.at_uri}} aria-label={`Explore ${item.title}`} className="rounded-full border border-line p-2 text-cyan"><ArrowUpRight className="size-4" /></Link></div>
  </article>
}
export function SongCards({ rows }: { rows: Ranking[] }) {
  return <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">{rows.map((row, index) => <Link key={row.id} to="/music/$id" params={{id:row.id}} className="discovery-card flex min-w-0 items-center gap-4 p-4"><Artwork url={row.artwork_url} className="size-16" /><div className="min-w-0"><p className="text-[10px] uppercase tracking-widest text-dim">{index+1} · Shared taste</p><h3 className="mt-1 line-clamp-2 break-words font-semibold">{row.title}</h3><p className="mt-1 truncate text-xs text-dim">{row.subtitle}</p><p className="mt-2 text-xs text-cyan">{row.listeners} {row.listeners === 1 ? 'listener' : 'listeners'} · {row.plays} plays</p></div></Link>)}</div>
}
export function Freshness({ at }: { at: number }) {
  const stale = Date.now()/1000-at > 1800
  return <p className={`text-xs leading-5 ${stale ? 'text-amber-200' : 'text-dim'}`}>{stale ? 'Showing an older selection · ' : 'Observed public activity · '}Updated {new Date(at*1000).toLocaleString([], {dateStyle:'medium',timeStyle:'short'})}. <Link to="/coverage" className="underline decoration-line underline-offset-4">About our coverage</Link></p>
}
export function RetryNotice({ retry, text = 'This selection is taking a little longer to load.' }: {retry:()=>void; text?:string}) { return <div role="alert" className="panel p-6 text-sm text-dim">{text} <button className="text-cyan underline" onClick={retry}>Try again</button></div> }
