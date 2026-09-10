import { useMemo, useState } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { createColumnHelper, flexRender, getCoreRowModel, getFilteredRowModel, getPaginationRowModel, getSortedRowModel, type SortingState, useReactTable } from '@tanstack/react-table'
import { Activity, ArrowDownUp, Bot, Check, ChevronRight, CircleAlert, Copy, ExternalLink, Gauge, Network, Search, TrendingUp, UserRound } from 'lucide-react'
import { Link, useNavigate } from '@tanstack/react-router'
import { Chart } from '@tanstack/charts/react'
import { defineChart, lineY } from '@tanstack/charts'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { tooltip } from '@tanstack/charts/tooltip'
import { api } from './api'
import { RecordPage } from './RecordDrawer'
import { FirehoseView } from './FirehoseView'
import { TrendPage } from './TrendPage'
import { CommandPalette } from './CommandPalette'
import type { AccountDetail, AccountEvent, Anomalies, ClusterDetail, Detection, Identity, Rates, StorageStats, SystemSample, SystemStats, TrendingItem } from './types'

const n = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 })
const compact = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })
const bytes = (value?:number) => {
  if (value == null || !Number.isFinite(value)) return '—'
  const units = ['B','KiB','MiB','GiB','TiB','PiB']; let i=0; let x=Math.max(0,value)
  while (x >= 1024 && i < units.length-1) { x/=1024; i++ }
  return `${x.toLocaleString(undefined,{maximumFractionDigits:i ? 2 : 0})} ${units[i]}`
}
const duration = (seconds?:number) => seconds == null ? '—' : seconds < 3600 ? `${Math.floor(seconds/60)}m` : seconds < 86400 ? `${(seconds/3600).toFixed(1)}h` : `${(seconds/86400).toFixed(1)}d`
const clock = (value:number) => new Date(value).toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',second:'2-digit'})
const pct = (x?:number) => x == null ? '—' : `${(x*(x<=1?100:1)).toFixed(1)}%`
const shortDid = (did:string) => did.length > 30 ? `${did.slice(0,18)}…${did.slice(-7)}` : did
const isDid = (value?:string) => Boolean(value?.startsWith('did:'))

function AccountIdentity({did,identities,onOpen,compact=false}:{did:string;identities:Map<string,Identity>;onOpen:(did:string)=>void;compact?:boolean}) {
  if(!isDid(did)) return <span className="min-w-0 truncate font-mono text-dim" title={did}>{did}</span>
  const handle=identities.get(did)?.handle
  return <button type="button" onClick={e=>{e.stopPropagation();onOpen(did)}} title={`${handle?`@${handle} · `:''}${did}`} className="group min-w-0 text-left text-cyan hover:text-mint focus:outline-none focus-visible:ring-2 focus-visible:ring-mint/60">
    <span className="block truncate font-medium">{handle?`@${handle}`:shortDid(did)}</span>
    {!compact&&handle&&<span className="block truncate font-mono text-[10px] text-dim group-hover:text-dim/80">{shortDid(did)}</span>}
  </button>
}

function AccountPage({did}:{did:string}) {
  const navigate=useNavigate()
  const account=useQuery({queryKey:['account',did],queryFn:()=>api<AccountDetail>(`/api/accounts/${encodeURIComponent(did)}`),staleTime:30_000})
  const storedEvents=useQuery({queryKey:['account-events',did],queryFn:()=>api<AccountEvent[]>(`/api/accounts/${encodeURIComponent(did)}/events`),staleTime:15_000})
  const identity=account.data?.current_identity; const activity=account.data?.activity; const [copied,setCopied]=useState(false)
  const copy=async()=>{await navigator.clipboard.writeText(did);setCopied(true);window.setTimeout(()=>setCopied(false),1500)}
  return <section className="panel mx-auto max-w-4xl p-6"><p className="eyebrow">Account activity</p><div className="mt-5 flex items-start gap-4"><div className="grid size-12 shrink-0 place-items-center rounded-2xl border border-mint/20 bg-mint/10"><UserRound className="size-6 text-mint"/></div><div className="min-w-0"><h2 className="truncate text-xl font-semibold text-cyan">{identity?.handle?`@${identity.handle}`:shortDid(did)}</h2><p className="mt-1 break-all font-mono text-[11px] text-dim">{did}</p></div></div><div className="mt-5 flex flex-wrap gap-2"><button onClick={copy} className="flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs hover:border-mint/50">{copied?<Check className="size-3.5 text-mint"/>:<Copy className="size-3.5"/>}{copied?'Copied':'Copy DID'}</button><a href={`https://bsky.app/profile/${encodeURIComponent(identity?.handle||did)}`} target="_blank" rel="noreferrer" className="flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs hover:border-cyan/50">Open on Bluesky <ExternalLink className="size-3.5"/></a></div>{account.isLoading?<Empty>Loading account activity…</Empty>:account.isError?<div className="mt-8 rounded-xl border border-amber-300/20 bg-amber-300/5 p-4 text-sm text-amber-200">Activity details are temporarily unavailable.</div>:activity&&<><div className="mt-8 grid grid-cols-2 gap-3 sm:grid-cols-4"><MiniStat label="Events · 24h" value={n.format(activity.events_24h)}/><MiniStat label="Posts · 24h" value={n.format(activity.posts_24h)}/><MiniStat label="Unique targets" value={n.format(activity.unique_targets)}/><MiniStat label="Automation signal" value={activity.automation_score.toFixed(2)}/></div><p className="mt-5 text-xs leading-relaxed text-dim">Scores describe observed activity patterns, not account identity or intent.</p><h3 className="eyebrow mb-3 mt-8">Recent observed events</h3><div className="space-y-2">{storedEvents.data?.length?storedEvents.data.map(event=>{const inspect=['post','reply','quote'].includes(event.kind);return <article key={event.event_id} className="rounded-xl border border-line p-4"><div className="flex items-center gap-2 text-xs"><span className="uppercase text-mint">{event.kind}</span><time className="text-dim">{new Date(event.time_us/1000).toLocaleString()}</time>{inspect&&<button className="ml-auto text-cyan hover:text-mint" onClick={()=>navigate({to:'/records/$uri',params:{uri:event.at_uri}})}>View record</button>}</div>{event.text&&<p className="mt-2 whitespace-pre-wrap text-sm text-white/85">{event.text}</p>}{!event.text&&event.target&&<p className="mt-2 truncate font-mono text-xs text-dim">{event.target}</p>}</article>}):<Empty>No events retained in the current hot window.</Empty>}</div></>}</section>
}

function MiniStat({label,value}:{label:string;value:string}) { return <div className="rounded-xl border border-line bg-panel p-4"><div className="text-xl font-semibold">{value}</div><div className="eyebrow mt-1">{label}</div></div> }
const didFromTarget = (target:string) => target.startsWith('at://') ? target.slice(5).split('/')[0] : target.startsWith('did:') ? target : undefined
function blueskyPostUrl(uri?:string,handles?:Map<string,string|undefined>) {
  if(!uri?.startsWith('at://')) return undefined
  const [did,collection,rkey]=uri.slice(5).split('/')
  if(collection!=='app.bsky.feed.post'||!rkey) return undefined
  return `https://bsky.app/profile/${encodeURIComponent(handles?.get(did)||did)}/post/${encodeURIComponent(rkey)}`
}

function EvidenceLink({uri,label,onOpen}:{uri?:string;label:string;onOpen:(uri:string)=>void}) {
  if(!blueskyPostUrl(uri))return null
  return <button onClick={()=>onOpen(uri!)} className="inline-flex items-center gap-1 text-cyan hover:text-mint">{label}<ChevronRight className="size-3"/></button>
}

function useApi<T>(key:string, path:string, enabled:boolean, interval:false|number=false) { return useQuery({ queryKey:[key], queryFn:()=>api<T>(path), enabled, refetchInterval:interval }) }
function useIdentityMap(values:string[]) {
  const dids=[...new Set(values.map(x=>didFromTarget(x)).filter((x):x is string=>Boolean(x)))].sort()
  const batches=Array.from({length:Math.ceil(dids.length/100)},(_,i)=>dids.slice(i*100,(i+1)*100))
  const queries=useQueries({queries:batches.map(batch=>({queryKey:['identities',...batch],queryFn:()=>api<Identity[]>(`/api/identities?dids=${encodeURIComponent(batch.join(','))}`),staleTime:300_000}))})
  return new Map(queries.flatMap(q=>q.data||[]).map(identity=>[identity.did,identity.handle]))
}
function IdentityLabel({value,handles,onOpen}:{value:string;handles:Map<string,string|undefined>;onOpen:(did:string)=>void}) {
  const did=didFromTarget(value),handle=did&&handles.get(did)
  if (!did) return <span title={value}>{value}</span>
  const suffix=value.startsWith('at://')?value.slice(`at://${did}`.length):''
  return <span className="min-w-0" title={value}><button type="button" onClick={()=>onOpen(did)} className="block max-w-full truncate font-medium text-cyan hover:text-mint">{handle?`@${handle}`:shortDid(did)}</button>{handle&&<span className="block truncate font-mono text-[10px] text-dim">{shortDid(did)}</span>}{suffix&&<span className="block truncate font-mono text-[10px] text-dim">{suffix}</span>}</span>
}

type GraphEdge = ClusterDetail['graph_edges'][number]

function InteractionGraph({edges,handles,onAccount}:{edges:GraphEdge[];handles:Map<string,string|undefined>;onAccount:(did:string)=>void}) {
  const [hovered,setHovered]=useState<string>()
  const nodes=useMemo(()=>{
    const weights=new Map<string,number>()
    edges.forEach(edge=>{
      weights.set(edge.source,(weights.get(edge.source)||0)+edge.event_count)
      weights.set(edge.target,(weights.get(edge.target)||0)+edge.event_count)
    })
    const ordered=[...weights].sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0]))
    const center={x:400,y:225}, radius=Math.min(165,80+ordered.length*7)
    return ordered.map(([id,weight],index)=>{
      const angle=-Math.PI/2+(index/ordered.length)*Math.PI*2
      return {id,weight,x:center.x+Math.cos(angle)*radius,y:center.y+Math.sin(angle)*radius}
    })
  },[edges])
  const positions=new Map(nodes.map(node=>[node.id,node]))
  const maxEdge=Math.max(...edges.map(edge=>edge.event_count),1)
  const maxNode=Math.max(...nodes.map(node=>node.weight),1)
  const label=(value:string)=>{const did=didFromTarget(value);return did?(handles.get(did)?`@${handles.get(did)}`:shortDid(did)):value}
  const activeEdges=hovered?edges.filter(edge=>edge.source===hovered||edge.target===hovered):[]
  const activeWeight=activeEdges.reduce((sum,edge)=>sum+edge.event_count,0)
  return <div className="overflow-hidden rounded-xl border border-line bg-black">
    <svg viewBox="0 0 800 450" className="block aspect-[16/9] min-h-72 w-full" role="img" aria-label={`Interaction network with ${nodes.length} accounts and ${edges.length} directed connections`}>
      <defs><marker id="graph-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="#ff8a1f"/></marker></defs>
      <g>
        {edges.map((edge,index)=>{const source=positions.get(edge.source),target=positions.get(edge.target);if(!source||!target)return null;const selected=!hovered||edge.source===hovered||edge.target===hovered;return <line key={`${edge.source}:${edge.target}:${index}`} x1={source.x} y1={source.y} x2={target.x} y2={target.y} stroke="#ff8a1f" strokeWidth={1+5*edge.event_count/maxEdge} strokeOpacity={selected ? .72 : .09} markerEnd="url(#graph-arrow)" className="transition-opacity"/>})}
      </g>
      <g>{nodes.map(node=>{const did=didFromTarget(node.id),selected=hovered===node.id,connected=!hovered||selected||activeEdges.some(edge=>edge.source===node.id||edge.target===node.id);const size=8+10*Math.sqrt(node.weight/maxNode);return <g key={node.id} transform={`translate(${node.x} ${node.y})`} role={did?'button':undefined} tabIndex={did?0:undefined} aria-label={`${label(node.id)}, ${node.weight} interactions`} onMouseEnter={()=>setHovered(node.id)} onMouseLeave={()=>setHovered(undefined)} onFocus={()=>setHovered(node.id)} onBlur={()=>setHovered(undefined)} onClick={()=>did&&onAccount(did)} onKeyDown={event=>{if(did&&(event.key==='Enter'||event.key===' ')){event.preventDefault();onAccount(did)}}} className={did?'cursor-pointer outline-none':'outline-none'} opacity={connected?1:.22}>
        <circle r={size+7} fill="transparent"/><circle r={size} fill={selected?'#ffb15c':'#160d06'} stroke={selected?'#fff2e5':'#ff8a1f'} strokeWidth={selected?3:2}/><text y={size+16} textAnchor="middle" fill={selected?'#fff2e5':'#cbbeb3'} fontSize="11" fontFamily="ui-monospace, monospace" paintOrder="stroke" stroke="#000" strokeWidth="4">{label(node.id).length>23?`${label(node.id).slice(0,21)}…`:label(node.id)}</text>
      </g>})}</g>
    </svg>
    <div className="flex min-h-11 flex-wrap items-center gap-x-5 gap-y-2 border-t border-line px-4 py-2 text-xs text-dim">
      <span><i className="mr-1.5 inline-block size-2.5 rounded-full border-2 border-mint"/>Account · size = interaction volume</span><span><i className="mr-1.5 inline-block h-0.5 w-5 bg-mint align-middle"/>Directed edge · width = event count</span>
      {hovered&&<span className="ml-auto text-white"><b className="text-cyan">{label(hovered)}</b> · {n.format(activeWeight)} connected events · click for account</span>}
    </div>
  </div>
}

function Stat({label,value,unit,icon:Icon,tone='mint'}:{label:string;value:string;unit?:string;icon:typeof Activity;tone?:'mint'|'cyan'|'amber'}) {
  const color=tone==='cyan'?'text-cyan':tone==='amber'?'text-amber-300':'text-mint'
  return <div className="panel p-5"><div className="flex items-center justify-between"><span className="eyebrow">{label}</span><Icon className={`${color} size-4`} /></div><div className="mt-4 flex items-baseline gap-2"><span className="text-3xl font-semibold tracking-tight">{value}</span>{unit&&<span className="text-xs text-dim">{unit}</span>}</div></div>
}

function Panel({title,caption,children,className=''}:{title:string;caption?:string;children:React.ReactNode;className?:string}) {
  return <section className={`panel min-w-0 p-5 ${className}`}><div className="mb-5"><h2 className="font-semibold tracking-tight">{title}</h2>{caption&&<p className="mt-1 text-xs text-dim">{caption}</p>}</div>{children}</section>
}

function Empty({children='Waiting for enough activity…'}:{children?:React.ReactNode}) { return <div className="flex min-h-28 items-center justify-center text-sm text-dim">{children}</div> }

function RateChart({rates}:{rates:Rates}) {
  const definition=useMemo(()=>{const rows=rates.windows.map(w=>({window:w.seconds/60, rate:w.events_per_sec})); return defineChart({marks:[lineY(rows,{id:'Event rate',x:'window',y:'rate',stroke:'#ff8a1f',points:true})],scales:{x:{scale:scaleLinear,axis:{label:'Window (minutes)'}},y:{scale:scaleLinear,axis:{label:'Events / second'}}},focus:'nearest-x',maxFocusDistance:Number.POSITIVE_INFINITY,tooltip:{use:tooltip,items:[{channel:'x',label:'Window',text:p=>`${n.format(Number(p.xValue))} min`},{channel:'y',label:'Event rate',text:p=>`${n.format(Number(p.yValue))} events/s`}]}})},[rates])
  return <Chart definition={definition} height={150} ariaLabel="Events per second by analysis window" />
}

function RankList({items,onOpen}:{items:TrendingItem[];onOpen?:(item:TrendingItem)=>void}) {
  if(!items.length)return <Empty />
  const max=Math.max(...items.map(x=>x.count),1)
  return <div className="space-y-4">{items.slice(0,8).map((item,i)=><button type="button" disabled={!onOpen} onClick={()=>onOpen?.(item)} className="block w-full text-left disabled:cursor-default" key={item.key}><div className="mb-1.5 flex items-center gap-3 text-sm"><span className="w-5 text-xs text-dim">{String(i+1).padStart(2,'0')}</span><span className="min-w-0 flex-1 truncate hover:text-orange-300" title={item.key}>{item.sample||item.key}</span><span className="font-mono text-xs text-mint">{compact.format(item.count)}</span></div><div className="ml-8 h-1 overflow-hidden rounded bg-line"><div className="h-full rounded bg-mint/70" style={{width:`${item.count/max*100}%`}} /></div></button>)}</div>
}

const clusterHelper=createColumnHelper<Detection>()
const clusterColumns=[
  clusterHelper.accessor('cluster_id',{header:'Cluster',cell:x=><span className="font-mono text-xs text-cyan">{x.getValue().replace('coord-','')}</span>}),
  clusterHelper.accessor(x=>x.accounts.length,{id:'accounts',header:'Accounts'}),
  clusterHelper.accessor('event_count',{header:'Events'}),
  clusterHelper.accessor(x=>x.scores.coordination_score,{id:'score',header:'Coordination',cell:x=><Score value={x.getValue()} />}),
  clusterHelper.accessor(x=>x.common_domains[0]||x.common_targets[0]||'—',{id:'evidence',header:'Primary evidence',cell:x=><span className="block max-w-60 truncate text-dim">{x.getValue()}</span>}),
]

function Score({value}:{value:number}) { return <div className="flex items-center gap-2"><div className="h-1.5 w-16 rounded bg-line"><div className="h-full rounded bg-cyan" style={{width:`${value*100}%`}} /></div><span className="font-mono text-xs">{value.toFixed(2)}</span></div> }

function ClusterTable({data,onOpen}:{data:Detection[];onOpen:(x:Detection)=>void}) {
  const [sorting,setSorting]=useState<SortingState>([{id:'score',desc:true}]); const [filter,setFilter]=useState('')
  const table=useReactTable({data,columns:clusterColumns,state:{sorting,globalFilter:filter},initialState:{pagination:{pageIndex:0,pageSize:10}},onSortingChange:setSorting,onGlobalFilterChange:setFilter,getCoreRowModel:getCoreRowModel(),getSortedRowModel:getSortedRowModel(),getFilteredRowModel:getFilteredRowModel(),getPaginationRowModel:getPaginationRowModel()})
  return <><div className="relative mb-4 max-w-xs"><Search className="absolute left-3 top-2.5 size-4 text-dim"/><input value={filter} onChange={e=>setFilter(e.target.value)} placeholder="Filter clusters…" className="w-full rounded-lg border border-line bg-ink/60 py-2 pl-9 pr-3 text-sm outline-none focus:border-mint/60" /></div><div className="scrollbar overflow-x-auto"><table className="w-full min-w-180 text-left text-sm"><thead className="border-b border-line text-xs text-dim">{table.getHeaderGroups().map(h=><tr key={h.id}>{h.headers.map(x=><th key={x.id} className="px-3 py-3 font-medium"><button className="flex items-center gap-1" onClick={x.column.getToggleSortingHandler()}>{flexRender(x.column.columnDef.header,x.getContext())}{x.column.getCanSort()&&<ArrowDownUp className="size-3"/>}</button></th>)}<th/></tr>)}</thead><tbody>{table.getRowModel().rows.map(row=><tr key={row.id} onClick={()=>onOpen(row.original)} className="cursor-pointer border-b border-line/60 hover:bg-white/[.025]">{row.getVisibleCells().map(cell=><td key={cell.id} className="px-3 py-3.5">{flexRender(cell.column.columnDef.cell,cell.getContext())}</td>)}<td><ChevronRight className="size-4 text-dim"/></td></tr>)}</tbody></table>{!data.length&&<Empty>No active coordination clusters</Empty>}</div>{data.length>0&&<div className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs text-dim"><span>Page {table.getState().pagination.pageIndex+1} of {table.getPageCount()}</span><div className="flex items-center gap-2"><label>Rows <select value={table.getState().pagination.pageSize} onChange={e=>table.setPageSize(Number(e.target.value))} className="rounded border border-line bg-ink px-2 py-1.5 text-white">{[10,20,50].map(x=><option key={x}>{x}</option>)}</select></label><button disabled={!table.getCanPreviousPage()} onClick={()=>table.previousPage()} className="rounded border border-line px-3 py-1.5 text-white disabled:opacity-30">Previous</button><button disabled={!table.getCanNextPage()} onClick={()=>table.nextPage()} className="rounded border border-line px-3 py-1.5 text-white disabled:opacity-30">Next</button></div></div>}</>
}

function ClusterPage({clusterId,onAccount,onRecord}:{clusterId:string;onAccount:(did:string)=>void;onRecord:(uri:string)=>void}) {
  const {data:d,isLoading,isError}=useQuery({queryKey:['cluster',clusterId],queryFn:()=>api<ClusterDetail>(`/api/clusters/${encodeURIComponent(clusterId)}`)})
  const identityValues=d?[...d.accounts,...d.common_targets,...d.sample_events.flatMap(x=>[x.actor_did,x.at_uri||'',x.target||'',x.reply_parent||'',x.reply_root||'',x.quote_target||'']),...d.graph_edges.flatMap(x=>[x.source,x.target])]:[]
  const handles=useIdentityMap(identityValues)
  if(isLoading)return <Empty>Loading detection…</Empty>
  if(isError||!d)return <Empty>Detection not found</Empty>
  const graphEdges=d.graph_edges.filter(x=>x.source!==x.target)
  const selfEdges=d.graph_edges.length-graphEdges.length
  return <section className="panel mx-auto max-w-5xl p-6"><p className="eyebrow">Detection evidence</p><h2 className="mt-2 font-mono text-xl text-cyan">{d.cluster_id}</h2><p className="mt-2 text-xs text-dim">Observed {new Date(d.first_seen).toLocaleString()} — {new Date(d.last_seen).toLocaleString()}</p>
    <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-5">{Object.entries(d.scores).map(([k,v])=><div className="rounded-xl border border-line p-3" key={k}><div className="text-lg font-semibold">{v.toFixed(2)}</div><div className="mt-1 text-[10px] uppercase tracking-wide text-dim">{k.replaceAll('_',' ')}</div></div>)}</div>
    <h3 className="eyebrow mt-8 mb-3">Participating identities · {d.accounts.length}</h3><div className="flex flex-wrap gap-2">{d.accounts.map(x=><div key={x} className="max-w-full rounded-xl border border-line bg-panel px-3 py-2 text-xs"><AccountIdentity did={x} identities={new Map([[x,{did:x,handle:handles.get(x),observed_at:'',source:''}]])} onOpen={onAccount}/></div>)}</div>
    <div className="mt-8 grid gap-5 sm:grid-cols-2"><div><h3 className="eyebrow mb-3">Evidence summary</h3><dl className="space-y-2 text-sm"><Row k="Shared target ratio" v={pct(d.shared_target_ratio)}/><Row k="Median timing" v={`${n.format(d.median_timing_difference_ms)} ms`}/><Row k="Repeated content" v={n.format(d.repeated_content_count)}/><Row k="Events" v={n.format(d.event_count)}/></dl></div><div><h3 className="eyebrow mb-3">Common domains</h3>{d.common_domains.length?<div className="flex flex-wrap gap-2">{d.common_domains.map(x=><span className="rounded bg-mint/10 px-2 py-1 text-xs text-mint" key={x}>{x}</span>)}</div>:<span className="text-sm text-dim">None</span>}</div></div>
    <h3 className="eyebrow mt-8 mb-3">Interaction graph</h3>{graphEdges.length?<><InteractionGraph edges={graphEdges} handles={handles} onAccount={onAccount}/>{selfEdges>0&&<p className="mt-3 text-xs text-dim">{selfEdges} self-interaction {selfEdges===1?'edge':'edges'} omitted from the coordination graph.</p>}</>:<Empty>{selfEdges?'Only self-interactions were retained; these are not coordination evidence.':'No directed edges retained'}</Empty>}
    <h3 className="eyebrow mt-8 mb-3">Sample events</h3><div className="space-y-2">{d.sample_events.map(x=><article key={x.event_id} className="rounded-lg border border-line p-3 text-xs"><div className="flex flex-wrap items-center gap-2"><span className="uppercase text-mint">{x.kind}</span><button onClick={()=>onAccount(x.actor_did)} className="truncate font-medium text-cyan hover:text-mint">{handles.get(x.actor_did)?`@${handles.get(x.actor_did)}`:shortDid(x.actor_did)}</button><span className="text-dim">· {new Date(x.timestamp).toLocaleTimeString()}</span><span className="ml-auto"><EvidenceLink uri={x.at_uri||x.target} label="View post" onOpen={onRecord}/></span></div>{x.text&&<p className="mt-2 line-clamp-3 leading-relaxed text-white/80">{x.text}</p>}<div className="mt-2 flex flex-wrap gap-3"><EvidenceLink uri={x.target} label="Target" onOpen={onRecord}/><EvidenceLink uri={x.reply_parent} label="Reply parent" onOpen={onRecord}/><EvidenceLink uri={x.reply_root} label="Thread root" onOpen={onRecord}/><EvidenceLink uri={x.quote_target} label="Quoted post" onOpen={onRecord}/></div></article>)}</div>
  </section>
}

function Row({k,v}:{k:string;v:string}) { return <div className="flex justify-between gap-4"><dt className="text-dim">{k}</dt><dd className="font-mono">{v}</dd></div> }

function SystemHistoryChart({history}:{history:SystemSample[]}) {
 const definition=useMemo(()=>{const rows=history.map(x=>({time:x.timestamp,rate:x.events_per_second}));return defineChart({marks:[lineY(rows,{id:'Event throughput',x:'time',y:'rate',stroke:'#ffb15c',points:true})],scales:{x:{scale:scaleLinear,axis:{label:'Time',ticks:{format:(v:number)=>clock(v)}}},y:{scale:scaleLinear,axis:{label:'Events / second',ticks:{format:(v:number)=>compact.format(v)}}}},focus:'nearest-x',maxFocusDistance:Number.POSITIVE_INFINITY,tooltip:{use:tooltip,items:[{channel:'x',label:'Observed',text:p=>clock(Number(p.xValue))},{channel:'y',label:'Throughput',text:p=>`${compact.format(Number(p.yValue))} events/s`}]}})},[history])
 return history.length>1?<Chart definition={definition} height={170} ariaLabel="Live event throughput history"/>:<Empty>Collecting history samples…</Empty>
}

function CpuHistoryChart({history}:{history:SystemSample[]}) {
 const definition=useMemo(()=>{const rows=history.filter(x=>x.host_cpu_utilization_percent!=null).map(x=>({time:x.timestamp,host:x.host_cpu_utilization_percent||0,process:x.process_cpu_utilization_percent||0,container:x.container_cpu_utilization_percent||0}));return defineChart({marks:[lineY(rows,{id:'Host CPU',x:'time',y:'host',stroke:'#ff8a1f',points:true}),lineY(rows,{id:'Container CPU',x:'time',y:'container',stroke:'#ffb15c',points:true}),lineY(rows,{id:'Rust process CPU',x:'time',y:'process',stroke:'#ffd0a3',points:true})],scales:{x:{scale:scaleLinear,axis:{label:'Time',ticks:{format:(v:number)=>clock(v)}}},y:{scale:scaleLinear,axis:{label:'CPU utilization',ticks:{format:(v:number)=>`${n.format(v)}%`}}}},focus:'group-x',maxFocusDistance:Number.POSITIVE_INFINITY,tooltip})},[history])
 return history.filter(x=>x.host_cpu_utilization_percent!=null).length>1?<><Chart definition={definition} height={170} ariaLabel="Host, container and process CPU utilization history"/><Legend labels={[['Host','#ff8a1f'],['Container','#ffb15c'],['Process','#ffd0a3']]}/></>:<Empty>Collecting CPU samples…</Empty>
}

function MemoryHistoryChart({history}:{history:SystemSample[]}) {
 const definition=useMemo(()=>{const rows=history.map(x=>({time:x.timestamp,process:x.process_resident_memory_bytes,container:x.container_memory_used_bytes||0}));return defineChart({marks:[lineY(rows,{id:'Container memory',x:'time',y:'container',stroke:'#ffb15c',points:true}),lineY(rows,{id:'Process resident memory',x:'time',y:'process',stroke:'#ffd0a3',points:true})],scales:{x:{scale:scaleLinear,axis:{label:'Time',ticks:{format:(v:number)=>clock(v)}}},y:{scale:scaleLinear,axis:{label:'Memory',ticks:{format:(v:number)=>bytes(v)}}}},focus:'group-x',maxFocusDistance:Number.POSITIVE_INFINITY,tooltip:{use:tooltip,items:[{channel:'x',label:'Observed',text:p=>clock(Number(p.xValue))},{channel:'y',label:'Memory',text:p=>bytes(Number(p.yValue))}]}})},[history])
 return history.length>1?<><Chart definition={definition} height={170} ariaLabel="Container and process memory history"/><Legend labels={[['Container memory','#ffb15c'],['Process resident memory','#ffd0a3']]}/></>:<Empty>Collecting memory samples…</Empty>
}

function Legend({labels}:{labels:[string,string][]}) { return <div className="mt-2 flex flex-wrap justify-center gap-4 text-[10px] uppercase tracking-wide text-dim">{labels.map(([label,color])=><span className="flex items-center gap-1.5" key={label}><i className="size-2 rounded-full" style={{background:color}}/>{label}</span>)}</div> }

function SystemView({storage,system,history}:{storage?:StorageStats;system?:SystemStats;history:SystemSample[]}) {
 const c=system?.clickhouse, tables=c?.tables||[],latest=history.at(-1); const chRows=tables.reduce((a,x)=>a+x.rows,0),chParts=tables.reduce((a,x)=>a+x.active_parts,0),chCompressed=tables.reduce((a,x)=>a+x.compressed_bytes,0),chRaw=tables.reduce((a,x)=>a+x.uncompressed_bytes,0)
 return <div className="grid gap-5 lg:grid-cols-3"><Panel title="Storage footprint" caption="Binary units; measured from live disk usage"><dl className="space-y-3 text-sm"><Row k="Raw event archive" v={bytes(storage?.raw_archive_bytes)}/><Row k="ClickHouse" v={bytes(storage?.clickhouse_bytes)}/><Row k="Derived detections" v={bytes(storage?.derived_bytes)}/><Row k="Daily growth" v={bytes(storage?.bytes_ingested_per_day)}/><Row k="Monthly projection" v={bytes(storage?.projected_monthly_bytes)}/><Row k="Yearly projection" v={bytes(storage?.projected_yearly_bytes)}/></dl></Panel>
 <Panel title="Storage array & runtime" caption={system?.storage_array.path||'Archive mount capacity'}><dl className="space-y-3 text-sm"><Row k="Used / available" v={`${bytes(system?.storage_array.used_bytes)} / ${bytes(system?.storage_array.available_bytes)}`}/><Row k="Process / container memory" v={`${bytes(system?.runtime.process_resident_memory_bytes)} / ${bytes(system?.runtime.container_memory_used_bytes)}`}/><Row k="Host CPU" v={(system?.runtime.host_cpu_utilization_percent??latest?.host_cpu_utilization_percent)==null?'—':`${n.format(system?.runtime.host_cpu_utilization_percent??latest!.host_cpu_utilization_percent!)}%`}/><Row k="Container CPU" v={(system?.runtime.container_cpu_utilization_percent??latest?.container_cpu_utilization_percent)==null?'—':`${n.format(system?.runtime.container_cpu_utilization_percent??latest!.container_cpu_utilization_percent!)}%`}/><Row k="Process CPU" v={(system?.runtime.process_cpu_utilization_percent??latest?.process_cpu_utilization_percent)==null?'—':`${n.format(system?.runtime.process_cpu_utilization_percent??latest!.process_cpu_utilization_percent!)}%`}/><Row k="Service uptime" v={duration(system?.runtime.process_uptime_seconds)}/></dl></Panel>
 <Panel title="ClickHouse" caption={system?.clickhouse_error||'Analytical datastore activity'}><dl className="space-y-3 text-sm"><Row k="Rows" v={c?compact.format(chRows):'—'}/><Row k="Parts / tables" v={c?`${n.format(chParts)} / ${n.format(tables.length)}`:'—'}/><Row k="Compressed" v={bytes(c?chCompressed:undefined)}/><Row k="Uncompressed" v={bytes(c?chRaw:undefined)}/><Row k="Compression ratio" v={chCompressed?`${n.format(chRaw/chCompressed)}×`:'—'}/><Row k="Queries / mutations" v={c?`${n.format(c.active_queries)} / ${n.format(c.pending_mutations)}`:'—'}/></dl></Panel>
 <Panel title="Throughput history" caption="Events/sec · 15-second samples" className="lg:col-span-3"><SystemHistoryChart history={history}/></Panel><Panel title="CPU utilization" caption="Host, cgroup, and Rust process percentages" className="lg:col-span-2"><CpuHistoryChart history={history}/></Panel><Panel title="Memory utilization" caption="Container usage and process RSS"><MemoryHistoryChart history={history}/></Panel></div>
}

export type AppRoute = {view:'network'|'firehose'|'anomalies'|'system'} | {view:'account';did:string} | {view:'record';uri:string} | {view:'cluster';clusterId:string} | {view:'trend';dimension:string;trendKey:string}

export default function App({route}:{route:AppRoute}) {
 const navigate=useNavigate()
 const openAccount=(did:string)=>navigate({to:'/accounts/$did',params:{did}})
 const openRecord=(uri:string)=>navigate({to:'/records/$uri',params:{uri}})
 const openCluster=(cluster:Detection)=>navigate({to:'/clusters/$clusterId',params:{clusterId:cluster.cluster_id}})
 const openTrend=(dimension:string,item:TrendingItem)=>navigate({to:'/trends/$dimension',params:{dimension},search:{key:item.key}})
 const network=route.view==='network', detections=route.view==='anomalies', systemPage=route.view==='system'
 const rates=useApi<Rates>('rates','/api/stats',network); const urls=useApi<[string,number][]>('urls','/api/trending/urls',network); const domains=useApi<[string,number][]>('domains','/api/trending/domains',network); const prints=useApi<TrendingItem[]>('prints','/api/trending/fingerprints',network); const conversations=useApi<TrendingItem[]>('conversations','/api/trending/conversations',network); const clusters=useApi<Detection[]>('clusters','/api/clusters',detections); const anomalies=useApi<Anomalies>('anomalies','/api/anomalies',detections); const storage=useApi<StorageStats>('storage','/api/storage',network||systemPage); const system=useApi<SystemStats>('system','/api/system',systemPage,15000); const history=useApi<SystemSample[]>('system-history','/api/system/history',systemPage,15000)
 const anomalyHandles=useIdentityMap((anomalies.data?.unusual_posting||[]).map(x=>x.did))
 const r=rates.data
 const normalized=(x?:[string,number][]):TrendingItem[] => (x||[]).map(([key,count])=>({key,count,recent_count:count,growth:0}))
 return <><header className="sticky top-0 z-30 border-b border-line/80 bg-ink/90 backdrop-blur-xl"><div className="mx-auto flex max-w-7xl items-center justify-between px-5 py-4"><div className="flex items-center gap-3"><div className="grid size-9 place-items-center rounded-xl bg-mint text-ink"><Network className="size-5"/></div><div><h1 className="text-sm font-semibold tracking-tight">Bluesky Observatory</h1><p className="text-[10px] uppercase tracking-[.16em] text-dim">Live ATProto network intelligence</p></div></div><div className="flex items-center gap-3"><CommandPalette/><div className="flex items-center gap-2 text-xs text-dim"><span className={`size-2 rounded-full ${rates.isError?'bg-red-400':'animate-pulse bg-mint'}`}/>{rates.isError?'Disconnected':'Firehose live'}</div></div></div></header>
 <main className="mx-auto max-w-7xl px-5 py-7"><nav className="mb-7 flex gap-1 overflow-x-auto rounded-xl border border-line bg-panel/70 p-1 sm:w-fit">{([['/','Network','network'],['/firehose','Firehose','firehose'],['/detections','Detections','anomalies'],['/system','System','system']] as const).map(([to,label,id])=><Link key={to} to={to} activeOptions={{exact:true}} className={`shrink-0 rounded-lg px-4 py-2 text-xs font-medium transition ${route.view===id?'bg-mint text-ink':'text-dim hover:text-white'}`}>{label}</Link>)}</nav>
 {route.view==='network'&&<><div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4"><Stat label="Events / sec" value={r?n.format(r.events_per_sec):'—'} icon={Activity}/><Stat label="Posts / sec" value={r?n.format(r.posts_per_sec):'—'} icon={TrendingUp} tone="cyan"/><Stat label="Likes / sec" value={r?n.format(r.likes_per_sec):'—'} icon={Gauge}/><Stat label="Follows / sec" value={r?n.format(r.follows_per_sec):'—'} icon={Network} tone="cyan"/></div><div className="mt-5 grid gap-5 lg:grid-cols-3"><Panel title="Rate by window" caption="Events/sec across configured sliding windows" className="lg:col-span-2">{r?<RateChart rates={r}/>:<Empty/>}</Panel><Panel title="Storage now" caption="Archive and analytical footprint"><div className="text-3xl font-semibold">{bytes((storage.data?.raw_archive_bytes||0)+(storage.data?.clickhouse_bytes||0))}</div><div className="mt-5 space-y-2"><Row k="Archive" v={bytes(storage.data?.raw_archive_bytes)}/><Row k="ClickHouse" v={bytes(storage.data?.clickhouse_bytes)}/><Row k="Growth / day" v={bytes(storage.data?.bytes_ingested_per_day)}/></div></Panel></div><div className="mt-5 grid gap-5 md:grid-cols-2 lg:grid-cols-3"><Panel title="Top domains" caption="Most shared in retained hot state"><RankList items={normalized(domains.data)} onOpen={item=>openTrend('domain',item)}/></Panel><Panel title="Top URLs" caption="Shared destinations"><RankList items={normalized(urls.data)} onOpen={item=>openTrend('url',item)}/></Panel><Panel title="Text fingerprints" caption="Repeated normalized phrases"><RankList items={prints.data||[]} onOpen={item=>openTrend('fingerprint',item)}/></Panel><Panel title="Growing conversations" caption="Fastest recent reply growth" className="md:col-span-2 lg:col-span-3"><RankList items={conversations.data||[]}/></Panel></div></>}
 {route.view==='anomalies'&&<><div className="grid gap-4 sm:grid-cols-3"><Stat label="Active clusters" value={n.format(clusters.data?.length||0)} icon={Network}/><Stat label="Posting anomalies" value={n.format(anomalies.data?.unusual_posting.length||0)} icon={CircleAlert} tone="amber"/><Stat label="Content campaigns" value={n.format(anomalies.data?.repeated_content_campaigns.length||0)} icon={Bot} tone="cyan"/></div><Panel title="Coordination clusters" caption="Evidence-based behavioural signals — never identity labels" className="mt-5"><ClusterTable data={clusters.data||[]} onOpen={openCluster}/></Panel><div className="mt-5 grid gap-5 lg:grid-cols-2"><Panel title="Unusual posting behaviour">{anomalies.data?.unusual_posting.length?<div className="max-h-120 space-y-3 overflow-y-auto pr-1 scrollbar">{anomalies.data.unusual_posting.map(a=><button onClick={()=>openAccount(a.did)} key={a.did} className="block w-full rounded-xl border border-line p-3 text-left hover:border-mint/50"><div className="flex justify-between gap-4"><span className="min-w-0 text-xs text-cyan" title={a.did}><b className="block truncate">{anomalyHandles.get(a.did)||shortDid(a.did)}</b>{anomalyHandles.get(a.did)&&<span className="block truncate font-mono text-[10px] text-dim">{a.did}</span>}</span><b>{a.automation_score.toFixed(2)}</b></div><p className="mt-2 text-xs text-dim">{a.events_24h} events · {a.posts_24h} posts · {a.unique_targets} targets</p></button>)}</div>:<Empty>No posting-rate anomalies</Empty>}</Panel><Panel title="Repeated-content campaigns">{anomalies.data?.repeated_content_campaigns.length?<div className="max-h-120 space-y-3 overflow-y-auto pr-1 scrollbar">{anomalies.data.repeated_content_campaigns.map(x=><button key={x.cluster_id} onClick={()=>openCluster(x)} className="flex w-full justify-between rounded-xl border border-line p-3 text-left hover:border-cyan/50"><span className="font-mono text-xs text-cyan">{x.cluster_id}</span><span className="text-xs">{x.repeated_content_count} matches</span></button>)}</div>:<Empty>No repeated-content campaigns</Empty>}</Panel></div></>}
 {route.view==='firehose'&&<FirehoseView onAccount={openAccount} onRecord={openRecord}/>} 
 {route.view==='system'&&<SystemView storage={storage.data} system={system.data} history={history.data||[]}/>} 
 {route.view==='account'&&<AccountPage did={route.did}/>} 
 {route.view==='record'&&<RecordPage uri={route.uri} onRecord={openRecord} onAccount={openAccount}/>} 
 {route.view==='cluster'&&<ClusterPage clusterId={route.clusterId} onAccount={openAccount} onRecord={openRecord}/>} 
 {route.view==='trend'&&<TrendPage dimension={route.dimension} trendKey={route.trendKey}/>} 
 </main></>
}
