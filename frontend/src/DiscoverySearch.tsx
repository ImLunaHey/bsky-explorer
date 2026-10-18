import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from './api'
import { apps, presentRecord, type Discovery, type Music } from './discovery'
import { ContentCards, SongCards } from './DiscoveryCards'

export function DiscoverySearch({query}:{query:string}) {
  const feed = useQuery({queryKey:['discovery'],queryFn:()=>api<Discovery>('/api/discover'),staleTime:60_000})
  const music = useQuery({queryKey:['music','7d'],queryFn:()=>api<Music>('/api/music?window=7d'),staleTime:60_000})
  const needle=query.toLowerCase().trim()
  const records=feed.data?.sections.flatMap(s=>s.records).filter(record=>{const item=presentRecord(record);return `${item.title} ${item.subtitle} ${item.body}`.toLowerCase().includes(needle)}).slice(0,6)||[]
  const songs=music.data?.tracks_by_listeners.filter(row=>`${row.title} ${row.subtitle}`.toLowerCase().includes(needle)).slice(0,6)||[]
  const foundApps=apps.filter(app=>`${app.name} ${app.description} ${app.category}`.toLowerCase().includes(needle))
  if (!needle) return null
  return <section className="my-6 space-y-4">{(records.length>0 || songs.length>0 || foundApps.length>0) && <><h2 className="text-lg font-semibold">From our discovery selection</h2><p className="text-xs text-dim">Matching recent highlights, weekly music picks and our app guide—not a full-content archive search.</p>{songs.length>0 && <SongCards rows={songs}/>}<ContentCards records={records}/><div className="flex flex-wrap gap-2">{foundApps.map(app=><Link key={app.id} to="/apps/$appId" params={{appId:app.id}} className="topic-chip">{app.name} →</Link>)}</div></>}{(feed.isError || music.isError) && <p className="text-xs text-dim">Some discovery results are unavailable. People and post search continues below.</p>}</section>
}
