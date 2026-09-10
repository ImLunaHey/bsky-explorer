import { useQuery } from '@tanstack/react-query'
import { ExternalLink, MessageCircle, Repeat2, Heart, Quote } from 'lucide-react'
import { api } from './api'
import type { Identity, RecordDetail, RecordItem } from './types'

const shortDid=(did:string)=>did.length>30?`${did.slice(0,18)}…${did.slice(-7)}`:did
const externalUrl=(uri:string,handle?:string)=>{const [did,,rkey]=uri.slice(5).split('/');return `https://bsky.app/profile/${encodeURIComponent(handle||did)}/post/${encodeURIComponent(rkey)}`}

function InternalLink({uri,onOpen,children}:{uri?:string;onOpen:(uri:string)=>void;children:string}) {
  if(!uri)return null
  return <button onClick={()=>onOpen(uri)} className="text-cyan hover:text-mint">{children}</button>
}

function RecordSummary({item,handles,onRecord,onAccount}:{item:RecordItem;handles:Map<string,string|undefined>;onRecord:(uri:string)=>void;onAccount:(did:string)=>void}) {
  return <article className="rounded-xl border border-line bg-panel/60 p-4"><div className="flex flex-wrap items-center gap-2 text-xs"><button onClick={()=>onAccount(item.actor_did)} className="font-medium text-cyan hover:text-mint">{handles.get(item.actor_did)?`@${handles.get(item.actor_did)}`:shortDid(item.actor_did)}</button><span className="uppercase text-mint">{item.kind}</span><time className="text-dim">{new Date(item.timestamp_ms).toLocaleString()}</time></div>{item.text&&<p className="mt-3 whitespace-pre-wrap break-words text-sm leading-relaxed text-white/90">{item.text}</p>}<div className="mt-3 flex flex-wrap gap-3 text-xs"><InternalLink uri={item.reply_parent} onOpen={onRecord}>Reply parent</InternalLink><InternalLink uri={item.reply_root} onOpen={onRecord}>Thread root</InternalLink><InternalLink uri={item.quote_target} onOpen={onRecord}>Quoted post</InternalLink></div>{item.urls.length>0&&<div className="mt-3 space-y-1">{item.urls.map(url=><a key={url} href={url} target="_blank" rel="noreferrer" className="block truncate text-xs text-cyan hover:text-mint">{url}</a>)}</div>}{item.languages.length>0&&<p className="mt-3 text-[10px] uppercase tracking-wide text-dim">Languages: {item.languages.join(', ')}</p>}</article>
}

export function RecordPage({uri,onRecord,onAccount}:{uri:string;onRecord:(uri:string)=>void;onAccount:(did:string)=>void}) {
  const detail=useQuery({queryKey:['record',uri],queryFn:()=>api<RecordDetail>(`/api/records?uri=${encodeURIComponent(uri)}`)})
  const dids=[...new Set(detail.data?[detail.data.record.actor_did,...detail.data.conversation.map(x=>x.actor_did),...detail.data.recent_interactions.map(x=>x.actor_did)]:[])]
  const identities=useQuery({queryKey:['identities',...dids],queryFn:()=>api<Identity[]>(`/api/identities?dids=${encodeURIComponent(dids.join(','))}`),enabled:dids.length>0,staleTime:300_000})
  const handles=new Map(identities.data?.map(x=>[x.did,x.handle])||[]),d=detail.data
  return <section className="panel mx-auto max-w-4xl p-6"><p className="eyebrow">Record detail</p>{detail.isLoading&&<div className="mt-16 text-center text-sm text-dim">Loading retained record…</div>}{detail.isError&&<div className="mt-8 rounded-xl border border-amber-300/20 bg-amber-300/5 p-4 text-sm text-amber-200">This record is not available in retained data.</div>}{d&&<><div className="mt-5"><RecordSummary item={d.record} handles={handles} onRecord={onRecord} onAccount={onAccount}/></div><div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">{([[Heart,'Likes',d.interaction_counts.likes],[Repeat2,'Reposts',d.interaction_counts.reposts],[MessageCircle,'Replies',d.interaction_counts.replies],[Quote,'Quotes',d.interaction_counts.quotes]] as const).map(([Icon,label,value])=><div key={label} className="rounded-xl border border-line p-3 text-center"><Icon className="mx-auto size-4 text-mint"/><b className="mt-2 block">{value}</b><span className="text-[10px] uppercase text-dim">{label}</span></div>)}</div>{d.conversation.length>0&&<><h3 className="eyebrow mt-8 mb-3">Conversation context</h3><div className="space-y-3">{d.conversation.map(x=><button key={x.at_uri} onClick={()=>onRecord(x.at_uri)} className="w-full text-left"><RecordSummary item={x} handles={handles} onRecord={onRecord} onAccount={onAccount}/></button>)}</div></>}<a href={externalUrl(uri,handles.get(d.record.actor_did))} target="_blank" rel="noreferrer" className="mt-8 inline-flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs hover:border-cyan/50">Open on Bluesky <ExternalLink className="size-3.5"/></a></>}</section>
}
