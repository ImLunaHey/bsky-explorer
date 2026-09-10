import { useEffect, useMemo, useState } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { Activity, CirclePause, CirclePlay, ExternalLink, Filter, Search, Wifi, WifiOff } from 'lucide-react'
import { api } from './api'
import type { LiveConnection } from './live'
import type { FirehoseEvent, FirehoseFeed, Identity } from './types'

const shortDid=(did:string)=>did.length>30?`${did.slice(0,18)}…${did.slice(-7)}`:did
const recordUri=(event:FirehoseEvent)=>['post','reply','quote'].includes(event.kind)&&event.at_uri?.includes('/app.bsky.feed.post/')?event.at_uri:undefined

function useHandles(events:FirehoseEvent[]) {
  const dids=[...new Set(events.flatMap(event=>[event.actor_did,event.target?.startsWith('did:')?event.target:undefined]).filter((x):x is string=>Boolean(x)))].sort()
  const batches=Array.from({length:Math.ceil(dids.length/100)},(_,i)=>dids.slice(i*100,(i+1)*100))
  const queries=useQueries({queries:batches.map(batch=>({queryKey:['identities',...batch],queryFn:()=>api<Identity[]>(`/api/identities?dids=${encodeURIComponent(batch.join(','))}`),staleTime:300_000,gcTime:3_600_000}))})
  return new Map(queries.flatMap(query=>query.data||[]).map(identity=>[identity.did,identity.handle]))
}

const tones:Record<string,string>={post:'bg-cyan/10 text-cyan',reply:'bg-cyan/10 text-cyan',like:'bg-pink-300/10 text-pink-300',repost:'bg-violet-300/10 text-violet-300',follow:'bg-mint/10 text-mint',unfollow:'bg-amber-300/10 text-amber-300',delete:'bg-red-300/10 text-red-300',quote:'bg-orange-300/10 text-orange-300'}

export function FirehoseView({onAccount,onRecord}:{onAccount:(did:string)=>void;onRecord:(uri:string)=>void}) {
  const [paused,setPaused]=useState(false)
  const [snapshot,setSnapshot]=useState<FirehoseFeed>()
  const [search,setSearch]=useState('')
  const [kinds,setKinds]=useState<Set<string>>(new Set())
  const connection=useQuery<LiveConnection>({queryKey:['live-connection'],queryFn:async()=>({state:'connecting'}),enabled:false})
  const live=connection.data?.state==='connected'
  const feed=useQuery({queryKey:['firehose-feed'],queryFn:()=>api<FirehoseFeed>('/api/firehose?limit=300'),refetchInterval:paused||live?false:3_000,refetchIntervalInBackground:true})
  useEffect(()=>{if(!paused&&feed.data)setSnapshot(feed.data)},[feed.data,paused])
  const data=paused?snapshot:feed.data
  const events=useMemo(()=>[...(data?.events||[])].reverse(),[data?.events])
  const handles=useHandles(events)
  const availableKinds=useMemo(()=>[...new Set(events.map(event=>event.kind))].sort(),[events])
  const needle=search.trim().toLocaleLowerCase()
  const visible=useMemo(()=>events.filter(event=>{
    if(kinds.size&& !kinds.has(event.kind))return false
    if(!needle)return true
    const handle=handles.get(event.actor_did)||''
    return [event.actor_did,handle,event.kind,event.text,event.at_uri,event.target,...(event.urls||[]),...(event.domains||[])].some(value=>value?.toLocaleLowerCase().includes(needle))
  }),[events,handles,kinds,needle])
  const toggleKind=(kind:string)=>setKinds(current=>{const next=new Set(current);if(next.has(kind))next.delete(kind);else next.add(kind);return next})

  return <div className="space-y-5">
    <section className="panel overflow-hidden">
      <div className="flex flex-col gap-4 border-b border-line p-5 lg:flex-row lg:items-center lg:justify-between">
        <div><div className="flex items-center gap-2"><h2 className="font-semibold">Live firehose</h2><span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-wide ${paused?'bg-amber-300/10 text-amber-300':connection.data?.state==='disconnected'&&feed.isError?'bg-red-300/10 text-red-300':'bg-mint/10 text-mint'}`}>{paused?<CirclePause className="size-3"/>:connection.data?.state==='disconnected'&&feed.isError?<WifiOff className="size-3"/>:<Wifi className="size-3"/>}{paused?'Paused':live?'Streaming':feed.isError?'Reconnecting':'Connecting'}</span></div><p className="mt-1 text-xs text-dim">Newest public ATProto events retained in the in-memory live buffer.</p></div>
        <div className="flex items-center gap-3"><span className="text-xs text-dim">Showing {visible.length} of {events.length}</span><button type="button" onClick={()=>setPaused(value=>!value)} className="inline-flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs font-medium hover:border-mint/50">{paused?<CirclePlay className="size-4 text-mint"/>:<CirclePause className="size-4"/>}{paused?'Resume live':'Pause'}</button></div>
      </div>
      <div className="space-y-3 border-b border-line bg-panel/40 p-4">
        <div className="relative"><Search className="absolute left-3 top-2.5 size-4 text-dim"/><input value={search} onChange={event=>setSearch(event.target.value)} placeholder="Filter text, DID, handle, URL, or domain…" className="w-full rounded-lg border border-line bg-ink/60 py-2 pl-9 pr-3 text-sm outline-none focus:border-mint/60"/></div>
        <div className="flex flex-wrap items-center gap-2"><Filter className="size-3.5 text-dim"/><button onClick={()=>setKinds(new Set())} className={`rounded-full border px-2.5 py-1 text-[11px] ${!kinds.size?'border-mint/50 bg-mint/10 text-mint':'border-line text-dim'}`}>All</button>{availableKinds.map(kind=><button key={kind} onClick={()=>toggleKind(kind)} className={`rounded-full border px-2.5 py-1 text-[11px] capitalize ${kinds.has(kind)?'border-mint/50 bg-mint/10 text-mint':'border-line text-dim hover:text-white'}`}>{kind}</button>)}</div>
      </div>
      <div className="scrollbar max-h-[68vh] min-h-96 overflow-y-auto">
        {feed.isLoading&&!data?<Empty text="Connecting to the live buffer…"/>:feed.isError&&!data?<Empty text="The live feed is temporarily unavailable."/>:visible.length===0?<Empty text={events.length?'No events match these filters.':'Waiting for live activity…'}/>:<div className="divide-y divide-line/70">{visible.map(event=><EventRow key={event.event_id} event={event} handle={handles.get(event.actor_did)} onAccount={onAccount} onRecord={onRecord}/>)}</div>}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line px-5 py-3 text-[10px] uppercase tracking-wide text-dim"><span>{data?.retained_event_count??0} events retained · showing latest 300</span><span>{data?.next_after_time_us!=null?`Cursor ${data.next_after_time_us}`:'Live cursor pending'}</span></div>
    </section>
  </div>
}

function EventRow({event,handle,onAccount,onRecord}:{event:FirehoseEvent;handle?:string;onAccount:(did:string)=>void;onRecord:(uri:string)=>void}) {
  const uri=recordUri(event)
  return <article className="grid grid-cols-[auto_minmax(0,1fr)] gap-3 px-4 py-3 transition hover:bg-white/[.02] sm:grid-cols-[5rem_minmax(9rem,13rem)_minmax(0,1fr)_auto] sm:items-start">
    <span className="text-[11px] tabular-nums text-dim">{new Date(event.time_us/1_000).toLocaleTimeString()}</span>
    <button type="button" onClick={()=>onAccount(event.actor_did)} className="min-w-0 text-left text-xs text-cyan hover:text-mint"><b className="block truncate font-medium">{handle?`@${handle}`:shortDid(event.actor_did)}</b>{handle&&<span className="block truncate font-mono text-[9px] text-dim">{shortDid(event.actor_did)}</span>}</button>
    <div className="col-span-2 min-w-0 sm:col-span-1"><div className="flex flex-wrap items-center gap-2"><span className={`rounded px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider ${tones[event.kind]||'bg-white/5 text-dim'}`}>{event.kind}</span>{event.domains?.map(domain=><span key={domain} className="rounded bg-mint/5 px-1.5 py-0.5 text-[10px] text-mint">{domain}</span>)}</div>{event.text?<p className="mt-1.5 line-clamp-2 text-sm leading-relaxed text-white/80">{event.text}</p>:<p className="mt-1.5 truncate font-mono text-[10px] text-dim">{event.target||event.at_uri||event.event_id}</p>}</div>
    {uri&&<button type="button" onClick={()=>onRecord(uri)} className="col-start-2 inline-flex items-center justify-self-end gap-1 text-[11px] text-cyan hover:text-mint sm:col-auto">Inspect <ExternalLink className="size-3"/></button>}
  </article>
}

function Empty({text}:{text:string}) {return <div className="grid min-h-96 place-items-center p-8 text-sm text-dim"><div className="text-center"><Activity className="mx-auto mb-3 size-6 opacity-50"/>{text}</div></div>}
