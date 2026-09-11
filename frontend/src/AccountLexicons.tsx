/* eslint-disable react-refresh/only-export-components */
import { memo, useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, Database, ListTree } from 'lucide-react'
import { api } from './api'
import { IdentityLink, useIdentityMap } from './identities'

type CollectionStat={collection:string;event_count:number;first_seen_ms:number;last_seen_ms:number}
type CollectionSample={collection:string;at_uri:string;operation:string;timestamp_ms:number;record_json:string}
type Membership={listitem_uri:string;list_uri:string;creator_did:string;observed_at_ms:number}
type Inventory={collections:CollectionStat[];samples:CollectionSample[];inbound_list_memberships:Membership[]}
type Family={id:string;label:string;description:string}
const PAGE_SIZE=6

export function classifyLexicon(collection:string):Family {
  if(collection.startsWith('app.bsky.graph.list')||collection.includes('starterpack')||collection.startsWith('tools.ozone.'))return{id:'curation',label:'Lists & moderation',description:'Lists, list entries, starter packs, or moderation records'}
  if(collection.startsWith('com.whtwnd.')||collection.includes('whitewind'))return{id:'whitewind',label:'WhiteWind / blogs',description:'Long-form publishing records'}
  if(collection.startsWith('fm.rocksky.')||collection.includes('rocksky')||collection.includes('scrobble'))return{id:'music',label:'Rocksky / music',description:'Music and listening activity records'}
  if(collection.startsWith('chat.')||collection.includes('.community')||collection.includes('.chat'))return{id:'community',label:'Chats & communities',description:'Conversation or community records'}
  if(collection.includes('profile')||collection.includes('location'))return{id:'profile',label:'Profile & location',description:'Profile or location metadata'}
  if(collection.includes('.rpg')||collection.includes('.game'))return{id:'games',label:'RPGs & games',description:'Game-related records'}
  if(collection.startsWith('com.imlunahey.'))return{id:'imlunahey',label:'com.imlunahey',description:'Custom imlunahey lexicons'}
  if(collection.startsWith('app.bsky.'))return{id:'bluesky',label:'Bluesky social',description:'Bluesky posts, reactions, graph, and feed records'}
  return{id:'other',label:'Other ATProto lexicons',description:'Unclassified public repository records'}
}

function sampleLabel(sample:CollectionSample) {
  try {const record=JSON.parse(sample.record_json) as Record<string,unknown>;for(const key of ['name','title','displayName','text','description'])if(typeof record[key]==='string'&&record[key])return String(record[key]).slice(0,120)} catch { /* compact record may be unavailable */ }
  return sample.at_uri.split('/').at(-1)||sample.at_uri
}

export const AccountLexicons = memo(function AccountLexicons({did}:{did:string}) {
  const inventory=useQuery({queryKey:['account-lexicons',did],queryFn:()=>api<Inventory>(`/api/accounts/${encodeURIComponent(did)}/lexicons`),staleTime:60_000})
  const [page,setPage]=useState(0)
  const data=inventory.data
  const handles=useIdentityMap(data?.inbound_list_memberships.map(item=>item.creator_did)||[])
  const families=useMemo(()=>{
    const grouped=new Map<string,{family:Family;collections:CollectionStat[]}>()
    for(const collection of data?.collections||[]){const family=classifyLexicon(collection.collection),entry=grouped.get(family.id)||{family,collections:[]};entry.collections.push(collection);grouped.set(family.id,entry)}
    return [...grouped.values()].sort((a,b)=>b.collections.reduce((n,x)=>n+x.event_count,0)-a.collections.reduce((n,x)=>n+x.event_count,0))
  },[data])
  const shown=families.slice(page*PAGE_SIZE,(page+1)*PAGE_SIZE),pages=Math.max(1,Math.ceil(families.length/PAGE_SIZE))
  if(inventory.isLoading)return <div className="mt-8 grid min-h-32 place-items-center text-sm text-dim">Loading repository collections…</div>
  if(inventory.isError)return <div className="mt-8 rounded-xl border border-red-500/20 p-4 text-sm text-red-200">Repository collection details are temporarily unavailable.</div>
  return <section className="mt-8"><div className="mb-3 flex items-end justify-between gap-4"><div><h3 className="eyebrow">ATProto repository activity</h3><p className="mt-1 text-xs text-dim">Observed public records, grouped conservatively by lexicon family.</p></div><span className="text-xs text-dim">{data?.collections.length||0} collections</span></div>
    <div className="grid gap-3 lg:grid-cols-2">{shown.map(({family,collections})=><details key={family.id} className="rounded-xl border border-line bg-black/40 p-4" open={families.length<=4}><summary className="cursor-pointer list-none"><div className="flex items-start gap-3"><Database className="mt-0.5 size-4 shrink-0 text-orange-400"/><div className="min-w-0 flex-1"><div className="flex justify-between gap-3"><b className="text-sm">{family.label}</b><span className="text-xs text-orange-300">{collections.reduce((n,x)=>n+x.event_count,0).toLocaleString()} events</span></div><p className="mt-1 text-xs text-dim">{family.description}</p></div></div></summary><div className="mt-4 space-y-3 border-t border-line pt-3">{collections.map(collection=><div key={collection.collection}><div className="flex flex-wrap justify-between gap-2"><Link to="/search" search={{q:collection.collection,type:'all',offset:0}} className="break-all font-mono text-xs text-orange-300 hover:text-orange-200">{collection.collection}</Link><span className="text-[10px] text-dim">{new Date(collection.first_seen_ms).toLocaleDateString()} – {new Date(collection.last_seen_ms).toLocaleString()}</span></div><div className="mt-2 space-y-1">{data?.samples.filter(sample=>sample.collection===collection.collection).map(sample=><Link key={sample.at_uri} to="/search" search={{q:sample.at_uri,type:'all',offset:0}} title={sample.at_uri} className="flex min-w-0 justify-between gap-3 rounded bg-white/[.025] px-2 py-1.5 text-xs hover:bg-orange-500/5"><span className="truncate">{sampleLabel(sample)}</span><span className="shrink-0 uppercase text-dim">{sample.operation}</span></Link>)}</div></div>)}</div></details>)}</div>
    {families.length>PAGE_SIZE&&<div className="mt-3 flex items-center justify-end gap-2 text-xs text-dim"><span>Page {page+1} of {pages}</span><button disabled={!page} onClick={()=>setPage(value=>value-1)} className="rounded border border-line p-1.5 disabled:opacity-30"><ChevronLeft className="size-4"/></button><button disabled={page+1>=pages} onClick={()=>setPage(value=>value+1)} className="rounded border border-line p-1.5 disabled:opacity-30"><ChevronRight className="size-4"/></button></div>}
    <div className="mt-5 rounded-xl border border-line p-4"><div className="flex items-center gap-2"><ListTree className="size-4 text-orange-400"/><h3 className="text-sm font-semibold">Observed inbound list memberships</h3></div><p className="mt-1 text-xs text-dim">Lists created by others that currently include this DID in retained listitem history. This differs from lists the account created or populated above.</p>{data?.inbound_list_memberships.length?<div className="mt-4 grid gap-2 sm:grid-cols-2">{data.inbound_list_memberships.slice(0,50).map(item=><div key={item.listitem_uri} className="min-w-0 rounded-lg bg-white/[.025] p-3"><Link to="/search" search={{q:item.list_uri,type:'all',offset:0}} className="block truncate font-mono text-xs text-orange-300" title={item.list_uri}>{item.list_uri}</Link><div className="mt-2 flex items-end justify-between gap-2"><IdentityLink did={item.creator_did} handle={handles.get(item.creator_did)} className="max-w-56 text-xs"/><time className="shrink-0 text-[10px] text-dim">{new Date(item.observed_at_ms).toLocaleDateString()}</time></div></div>)}</div>:<p className="mt-4 text-sm text-dim">No retained inbound list memberships observed.</p>}</div>
  </section>
})
