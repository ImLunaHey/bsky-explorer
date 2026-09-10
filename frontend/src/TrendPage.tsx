import { useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { api } from './api'
import type { TrendDrilldown } from './types'

const PAGE_SIZE=25
const shortDid=(did:string)=>did.length>34?`${did.slice(0,20)}…${did.slice(-8)}`:did

export function TrendPage({dimension,trendKey}:{dimension:string;trendKey:string}) {
  const [page,setPage]=useState(0)
  const [kind,setKind]=useState('')
  const query=useQuery({
    queryKey:['trend-drilldown',dimension,trendKey,kind,page],
    queryFn:()=>api<TrendDrilldown>(`/api/trending/drilldown?dimension=${encodeURIComponent(dimension)}&key=${encodeURIComponent(trendKey)}&limit=${PAGE_SIZE}&offset=${page*PAGE_SIZE}${kind?`&kind=${encodeURIComponent(kind)}`:''}`),
    placeholderData:keepPreviousData,
  })
  const data=query.data
  const pages=Math.max(1,Math.ceil((data?.total||0)/PAGE_SIZE))
  return <section className="panel overflow-hidden">
    <div className="border-b border-line p-6"><p className="eyebrow">Trending evidence · {dimension}</p><h2 className="mt-2 break-all text-xl font-semibold">{trendKey}</h2><p className="mt-2 text-xs text-dim">Events in the retained live analysis window that contributed to this trend.</p></div>
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line p-4"><label className="text-xs text-dim">Event type <select value={kind} onChange={event=>{setKind(event.target.value);setPage(0)}} className="ml-2 rounded-lg border border-line bg-ink px-3 py-2 text-white"><option value="">All</option>{['post','reply','quote','like','repost','follow','unfollow','delete'].map(value=><option key={value}>{value}</option>)}</select></label><span className="text-xs text-dim">{data?.total??0} matching events</span></div>
    {query.isLoading?<div className="grid min-h-64 place-items-center text-sm text-dim">Loading evidence…</div>:query.isError?<div className="grid min-h-64 place-items-center text-sm text-red-300">Evidence is temporarily unavailable.</div>:!data?.events.length?<div className="grid min-h-64 place-items-center text-sm text-dim">No matching events in retained memory.</div>:<div className="overflow-x-auto"><table className="w-full min-w-190 text-left text-sm"><thead className="border-b border-line text-xs text-dim"><tr><th className="px-5 py-3">Time</th><th className="px-5 py-3">Type</th><th className="px-5 py-3">Account</th><th className="px-5 py-3">Evidence</th><th className="px-5 py-3"/></tr></thead><tbody>{data.events.map(event=>{const inspect=['post','reply','quote'].includes(event.kind)&&event.at_uri.includes('/app.bsky.feed.post/');return <tr key={event.event_id} className="border-b border-line/60 hover:bg-white/[.025]"><td className="whitespace-nowrap px-5 py-4 text-xs text-dim">{new Date(event.timestamp).toLocaleString()}</td><td className="px-5 py-4 uppercase text-orange-400">{event.kind}</td><td className="max-w-52 px-5 py-4"><Link to="/accounts/$did" params={{did:event.actor_did}} title={event.actor_did} className="block truncate font-mono text-xs text-orange-300 hover:text-orange-200">{shortDid(event.actor_did)}</Link></td><td className="max-w-lg px-5 py-4"><p className="line-clamp-2 text-white/80">{event.text||event.target||event.at_uri}</p>{event.domains.length>0&&<p className="mt-1 truncate text-[10px] text-dim">{event.domains.join(' · ')}</p>}</td><td className="px-5 py-4">{inspect&&<Link to="/records/$uri" params={{uri:event.at_uri}} className="text-xs text-orange-300 hover:text-orange-200">Open</Link>}</td></tr>})}</tbody></table></div>}
    {!!data?.cluster_ids.length&&<div className="border-t border-line p-5"><p className="eyebrow mb-3">Related detections</p><div className="flex flex-wrap gap-2">{data.cluster_ids.map(clusterId=><Link key={clusterId} to="/clusters/$clusterId" params={{clusterId}} className="rounded-lg border border-orange-500/30 px-3 py-2 font-mono text-xs text-orange-300 hover:bg-orange-500/10">{clusterId}</Link>)}</div></div>}
    <div className="flex items-center justify-between border-t border-line p-4 text-xs text-dim"><span>Page {page+1} of {pages}</span><div className="flex gap-2"><button disabled={page===0} onClick={()=>setPage(value=>Math.max(0,value-1))} className="inline-flex items-center rounded-lg border border-line px-3 py-2 text-white disabled:opacity-30"><ChevronLeft className="mr-1 size-3"/>Previous</button><button disabled={page+1>=pages} onClick={()=>setPage(value=>value+1)} className="inline-flex items-center rounded-lg border border-line px-3 py-2 text-white disabled:opacity-30">Next<ChevronRight className="ml-1 size-3"/></button></div></div>
  </section>
}
