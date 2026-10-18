import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from './api'

type Bucket = { timestamp_ms: number; gained: number; lost: number; net: number; followed: number; unfollowed: number; unknown_unfollows: number; posts: number; replies: number; likes: number; reposts: number; other: number; likes_received: number; reposts_received: number; replies_received: number }
type History = { start_ms: number; end_ms: number; bucket_ms: number; buckets: Bucket[]; note: string }
type Metric = keyof Omit<Bucket, 'timestamp_ms'>
type Series = { key: Metric; label: string; color: string }
const audience: Series[] = [{key:'gained',label:'Follows gained',color:'#67e8f9'},{key:'lost',label:'Known losses',color:'#fbbf24'},{key:'net',label:'Observed net',color:'#a78bfa'}]
const activity: Series[] = [{key:'posts',label:'Posts & quotes',color:'#67e8f9'},{key:'replies',label:'Replies',color:'#a78bfa'},{key:'likes',label:'Likes given',color:'#6ee7b7'},{key:'reposts',label:'Reposts',color:'#fbbf24'},{key:'other',label:'Other activity',color:'#94a3b8'}]
const engagement: Series[] = [{key:'likes_received',label:'Likes received',color:'#67e8f9'},{key:'reposts_received',label:'Reposts received',color:'#6ee7b7'},{key:'replies_received',label:'Replies received',color:'#a78bfa'}]
const following: Series[] = [{key:'followed',label:'Followed others',color:'#67e8f9'},{key:'unfollowed',label:'Unfollowed others',color:'#fbbf24'}]
const number = new Intl.NumberFormat()
const stamp = (ms:number, hourly:boolean) => new Date(ms).toLocaleString([], hourly ? {month:'short',day:'numeric',hour:'numeric'} : {month:'short',day:'numeric'})

function HistoryChart({title,description,data,series,hourly}:{title:string;description:string;data:Bucket[];series:Series[];hourly:boolean}) {
  const [selected,setSelected] = useState<number|null>(null)
  const maximum = Math.max(1,...data.flatMap(row=>series.map(s=>row[s.key])))
  const minimum = Math.min(0,...data.flatMap(row=>series.map(s=>row[s.key])))
  const y = (value:number) => 175-(value-minimum)/(maximum-minimum)*145
  const point = (index:number,value:number) => `${45+index*690/Math.max(1,data.length-1)},${y(value)}`
  const row = selected === null ? undefined : data[selected]
  return <section className="panel min-w-0 p-5">
    <h3 className="text-lg font-semibold">{title}</h3><p className="mt-1 text-xs leading-5 text-dim">{description}</p>
    <div className="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-xs">{series.map(s=><span key={s.key} style={{color:s.color}}>{s.label} · {number.format(data.reduce((n,row)=>n+row[s.key],0))}</span>)}</div>
    <svg viewBox="0 0 760 210" role="img" aria-label={`${title}. ${series.map(s=>`${s.label}: ${data.reduce((n,r)=>n+r[s.key],0)}`).join('. ')}. Exact values in the table below.`} className="mt-3 w-full">
      {[...new Set([minimum,0,Math.round((maximum+minimum)/2),maximum])].map(value=><g key={value}><line x1="45" x2="735" y1={y(value)} y2={y(value)} stroke="#334155" strokeDasharray="3 5"/><text x="38" y={y(value)+4} textAnchor="end" fill="#94a3b8" fontSize="11">{number.format(value)}</text></g>)}
      {series.map(s=><polyline key={s.key} fill="none" stroke={s.color} strokeWidth="2.5" points={data.map((row,i)=>point(i,row[s.key])).join(' ')}/>)}
      {data.map((row,i)=><g key={row.timestamp_ms} role="button" tabIndex={0} aria-label={`${stamp(row.timestamp_ms,hourly)}: ${series.map(s=>`${s.label} ${row[s.key]}`).join(', ')}`} onClick={()=>setSelected(i)} onKeyDown={e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();setSelected(i)}}} className="cursor-pointer focus:outline-cyan">
        <rect x={35+i*690/Math.max(1,data.length-1)} y="20" width="20" height="160" fill={selected===i?'#67e8f920':'transparent'}/>
        <circle cx={45+i*690/Math.max(1,data.length-1)} cy={y(row[series[0].key])} r="3" fill={series[0].color}/>
      </g>)}
      <text x="45" y="202" fill="#94a3b8" fontSize="11">{stamp(data[0].timestamp_ms,hourly)}</text><text x="735" y="202" textAnchor="end" fill="#94a3b8" fontSize="11">{stamp(data.at(-1)!.timestamp_ms,hourly)}</text>
    </svg>
    <p aria-live="polite" className="min-h-10 text-xs leading-5 text-dim">{row ? `${stamp(row.timestamp_ms,hourly)} — ${series.map(s=>`${s.label}: ${number.format(row[s.key])}`).join(' · ')}` : 'Select a point to inspect that period, or expand the exact values below.'}</p>
    <details className="mt-2 text-xs"><summary className="cursor-pointer text-cyan">Exact values</summary><div className="mt-3 overflow-x-auto"><table className="w-full text-left"><thead><tr><th className="py-2">Period</th>{series.map(s=><th className="px-3" key={s.key}>{s.label}</th>)}</tr></thead><tbody>{data.map(row=><tr className="border-t border-line" key={row.timestamp_ms}><th className="whitespace-nowrap py-2 font-normal">{stamp(row.timestamp_ms,hourly)}</th>{series.map(s=><td key={s.key} className="px-3">{number.format(row[s.key])}</td>)}</tr>)}</tbody></table></div></details>
  </section>
}

export function ProfileAnalytics({did,mode='overview'}:{did:string;mode?:'overview'|'audience'|'activity'}) {
  const [days,setDays]=useState(1)
  const query=useQuery({queryKey:['profile-history',did,days],queryFn:()=>api<History>(`/api/accounts/${encodeURIComponent(did)}/history?days=${days}`),staleTime:60_000,retry:1})
  const data=query.data
  const sum=(key:Metric)=>data?.buckets.reduce((n,row)=>n+row[key],0) || 0
  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-xl font-semibold">{mode==='audience'?'Audience changes':mode==='activity'?'Activity & response':'The account at a glance'}</h2><p className="mt-1 text-sm text-dim">Observed activity, with hourly or daily detail.</p></div><div className="flex gap-1 rounded-lg border border-line p-1" aria-label="History period">{[[1,'24 hours'],[7,'7 days'],[30,'30 days']].map(([value,label])=><button key={value} aria-pressed={days===value} onClick={()=>setDays(Number(value))} className={`rounded-md px-3 py-2 text-xs ${days===value?'bg-cyan text-ink':'text-dim'}`}>{label}</button>)}</div></div>
    {query.isPending && <div role="status" className="panel p-8 text-dim">Loading account history…</div>}
    {query.isError && <div role="alert" className="panel p-6 text-dim">History couldn’t load within the query budget. <button className="text-cyan" onClick={()=>void query.refetch()}>Try again</button></div>}
    {data && <>
      <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">{(mode==='activity' ? [['Posts & replies',sum('posts')+sum('replies')],['Likes received',sum('likes_received')],['Reposts received',sum('reposts_received')],['Replies received',sum('replies_received')]] : [['Follows gained',sum('gained')],['Known losses',sum('lost')],['Observed net',sum('gained')-sum('lost')],['Posts & replies',sum('posts')+sum('replies')]]).map(([label,value])=><div className="panel p-4" key={label}><p className="text-xs text-dim">{label}</p><p className="mt-2 text-3xl font-semibold tabular-nums">{number.format(Number(value))}</p></div>)}</div>
      <p className="text-xs leading-6 text-dim">{data.note}</p>
      <div className="grid items-start gap-5 xl:grid-cols-2">
        {mode!=='activity' && <HistoryChart key={`audience-${days}`} title="Followers gained & lost" description="Incoming follows and target-resolved unfollows—not total followers." data={data.buckets} series={audience} hourly={days===1}/>}
        {mode!=='audience' && <HistoryChart key={`activity-${days}`} title="What they’re doing" description="Public actions made by this account. Follow changes are shown separately." data={data.buckets} series={activity} hourly={days===1}/>}
        {mode==='audience' && <HistoryChart key={`following-${days}`} title="Who they’re following" description={`Outgoing follow changes. ${sum('unknown_unfollows')} removals have an unknown target.`} data={data.buckets} series={following} hourly={days===1}/>}
        {mode==='activity' && <HistoryChart key={`engagement-${days}`} title="Response to their work" description="Incoming interactions with a known target account; excludes self-interactions." data={data.buckets} series={engagement} hourly={days===1}/>}
      </div>
      <p className="text-xs text-dim">Through {new Date(data.end_ms).toLocaleString()} · {days===1?'Hourly':'Daily'} buckets · Counts are events, not unique people. <a href="/coverage" className="text-cyan">About coverage →</a></p>
    </>}
  </div>
}
