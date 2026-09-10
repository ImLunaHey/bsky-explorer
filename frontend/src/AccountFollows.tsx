import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { api } from './api'

export type FollowWindow = {
  seconds:number
  outbound_follows:number
  inbound_follows:number
  outbound_unfollows_resolved:number
  inbound_unfollows_resolved:number
  outbound_unfollows_unresolved:number
  outbound_net:number
  inbound_net:number
}

export type FollowEvent = {
  event_id:string
  timestamp_ms:number
  actor_did:string
  target_did:string
  kind:'follow'|'unfollow'
  direction:'inbound'|'outbound'
  target_resolved:boolean
}

export type FollowAnalytics = { windows:FollowWindow[];recent_events:FollowEvent[];unresolved_note:string }
type Identity = {did:string;handle?:string}
const number=new Intl.NumberFormat()
const short=(did:string)=>did.length>30?`${did.slice(0,18)}…${did.slice(-7)}`:did
const windowName=(seconds:number)=>seconds<3600?`${seconds/60}m`:seconds<86400?`${seconds/3600}h`:`${seconds/86400}d`

export function AccountFollows({did,onAccount}:{did:string;onAccount:(did:string)=>void}) {
  const [page,setPage]=useState(0),pageSize=10
  const analytics=useQuery({queryKey:['account-follows',did],queryFn:()=>api<FollowAnalytics>(`/api/accounts/${encodeURIComponent(did)}/follows`),staleTime:15_000})
  const identityDids=useMemo(()=>[...new Set((analytics.data?.recent_events||[]).flatMap(event=>[event.actor_did,event.target_did]).filter(Boolean))],[analytics.data])
  const identities=useQuery({queryKey:['identities',...identityDids.sort()],queryFn:()=>api<Identity[]>(`/api/identities?dids=${encodeURIComponent(identityDids.join(','))}`),enabled:identityDids.length>0,staleTime:300_000})
  const handles=new Map((identities.data||[]).map(identity=>[identity.did,identity.handle]))
  if(analytics.isLoading)return <div className="mt-8 flex min-h-28 items-center justify-center text-sm text-dim">Loading follow activity…</div>
  if(analytics.isError||!analytics.data)return <div className="mt-8 rounded-xl border border-red-400/20 p-4 text-sm text-red-300">Follow activity is temporarily unavailable.</div>
  const events=analytics.data.recent_events,pages=Math.max(1,Math.ceil(events.length/pageSize)),safePage=Math.min(page,pages-1),visible=events.slice(safePage*pageSize,(safePage+1)*pageSize)
  const accountLabel=(accountDid:string)=>handles.get(accountDid)?`@${handles.get(accountDid)}`:short(accountDid)
  return <>
    <h3 className="eyebrow mb-3 mt-8">Follow activity</h3>
    <div className="scrollbar overflow-x-auto rounded-xl border border-line"><table className="w-full min-w-175 text-left text-xs"><thead className="border-b border-line text-dim"><tr><th className="px-3 py-3">Window</th><th className="px-3">Following</th><th className="px-3">Followers</th><th className="px-3">Unfollowed</th><th className="px-3">Lost followers</th><th className="px-3">Unknown target</th><th className="px-3">Outbound net</th><th className="px-3">Inbound net</th></tr></thead><tbody>{analytics.data.windows.map(row=><tr key={row.seconds} className="border-b border-line/60 last:border-0"><th className="px-3 py-3 text-cyan">{windowName(row.seconds)}</th><td className="px-3">{number.format(row.outbound_follows)}</td><td className="px-3">{number.format(row.inbound_follows)}</td><td className="px-3">{number.format(row.outbound_unfollows_resolved)}</td><td className="px-3">{number.format(row.inbound_unfollows_resolved)}</td><td className="px-3 text-amber-300">{number.format(row.outbound_unfollows_unresolved)}</td><td className={`px-3 font-semibold ${row.outbound_net<0?'text-red-300':'text-mint'}`}>{row.outbound_net>0?'+':''}{number.format(row.outbound_net)}</td><td className={`px-3 font-semibold ${row.inbound_net<0?'text-red-300':'text-mint'}`}>{row.inbound_net>0?'+':''}{number.format(row.inbound_net)}</td></tr>)}</tbody></table></div>
    <p className="mt-3 text-xs leading-relaxed text-dim">{analytics.data.unresolved_note}</p>
    <h3 className="eyebrow mb-3 mt-8">Recent follows and unfollows</h3>
    {visible.length?<div className="space-y-2">{visible.map(event=>{const other=event.direction==='outbound'?event.target_did:event.actor_did;return <article key={event.event_id} className="flex flex-wrap items-center gap-2 rounded-xl border border-line p-3 text-xs"><span className={event.kind==='follow'?'text-mint':'text-amber-300'}>{event.direction==='inbound'?(event.kind==='follow'?'Followed by':'Unfollowed by'):(event.kind==='follow'?'Followed':'Unfollowed')}</span>{other?<button onClick={()=>onAccount(other)} title={other} className="font-medium text-cyan hover:text-mint">{accountLabel(other)}</button>:<span className="italic text-dim">target unavailable</span>}<time className="ml-auto text-dim">{new Date(event.timestamp_ms).toLocaleString()}</time></article>})}</div>:<div className="flex min-h-28 items-center justify-center text-sm text-dim">No recent follow activity retained.</div>}
    {events.length>pageSize&&<div className="mt-4 flex items-center justify-between text-xs text-dim"><span>Page {safePage+1} of {pages}</span><div className="flex gap-2"><button disabled={safePage===0} onClick={()=>setPage(Math.max(0,safePage-1))} className="rounded border border-line p-2 text-white disabled:opacity-30" aria-label="Previous page"><ChevronLeft className="size-4"/></button><button disabled={safePage>=pages-1} onClick={()=>setPage(Math.min(pages-1,safePage+1))} className="rounded border border-line p-2 text-white disabled:opacity-30" aria-label="Next page"><ChevronRight className="size-4"/></button></div></div>}
  </>
}
