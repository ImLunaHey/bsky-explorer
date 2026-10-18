import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from './api'
import type { AccountProfile } from './types'

export function AccountPreview({did,children}:{did:string;children:ReactNode}) {
  const host=useRef<HTMLSpanElement>(null),card=useRef<HTMLDivElement>(null)
  const timer=useRef<ReturnType<typeof setTimeout>|undefined>(undefined)
  const [position,setPosition]=useState<{left:number;top:number}>()
  const [copied,setCopied]=useState(false)
  const id=useId()
  const open=()=>{const rect=host.current?.getBoundingClientRect();if(rect)setPosition({left:Math.max(8,Math.min(rect.left,window.innerWidth-336)),top:Math.max(8,Math.min(rect.bottom+6,window.innerHeight-380))})}
  const close=()=>{clearTimeout(timer.current);setPosition(undefined)}
  useEffect(()=>()=>clearTimeout(timer.current),[])
  useEffect(()=>{
    if(!position)return
    const outside=(event:PointerEvent)=>{if(!host.current?.contains(event.target as Node)&&!card.current?.contains(event.target as Node))setPosition(undefined)}
    const escape=(event:KeyboardEvent)=>{if(event.key==='Escape'){setPosition(undefined);host.current?.querySelector<HTMLButtonElement>('button[data-preview]')?.focus()}}
    document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape)
    return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape)}
  },[position])
  const profile=useQuery({queryKey:['account-profile',did],queryFn:()=>api<AccountProfile>(`/api/accounts/${encodeURIComponent(did)}/profile`),enabled:!!position,staleTime:300_000})
  const person=profile.data
  return <span ref={host} className="inline-flex min-w-0 max-w-full items-start gap-1" onMouseEnter={()=>{clearTimeout(timer.current);timer.current=setTimeout(open,350)}} onMouseLeave={()=>{clearTimeout(timer.current);timer.current=setTimeout(()=>setPosition(undefined),250)}}>
    {children}<button type="button" data-preview aria-label={`Preview account ${did}`} aria-expanded={!!position} aria-controls={position?id:undefined} onClick={event=>{event.stopPropagation();if(position)close();else open()}} className="shrink-0 px-1 text-[10px] text-dim hover:text-cyan">ⓘ</button>
    {position&&createPortal(<div ref={card} id={id} role="dialog" aria-label="Account preview" onClick={event=>event.stopPropagation()} onMouseEnter={()=>clearTimeout(timer.current)} onMouseLeave={close} style={position} className="fixed z-50 max-h-[80vh] w-80 max-w-[calc(100vw-16px)] overflow-auto border border-line bg-ink p-4 text-left shadow-xl">
      <div className="flex justify-between gap-3"><span className="eyebrow">Account preview</span><button aria-label="Close account preview" onClick={close}>×</button></div>
      {profile.isPending?<p role="status" className="mt-3 animate-pulse text-xs text-dim">Loading public profile…</p>:person?<><div className="mt-3 flex items-center gap-3">{person.avatar_url&&<img src={person.avatar_url} alt="" loading="lazy" referrerPolicy="no-referrer" className="size-10 object-cover"/>}<div className="min-w-0"><b className="block truncate text-sm">{person.display_name||person.handle}</b><span className="block truncate text-xs text-cyan">@{person.handle}</span></div></div><p className="mt-3 line-clamp-4 whitespace-pre-wrap text-xs leading-5 text-dim">{person.description||'No public bio available.'}</p><p className="mt-3 text-xs">{person.followers_count?.toLocaleString()??'—'} followers · {person.posts_count?.toLocaleString()??'—'} posts</p><p className="mt-1 text-[10px] text-dim">Current Bluesky totals, not retained activity.</p></>:<p className="mt-3 text-xs text-dim">Public profile unavailable. The repository may still have retained activity.</p>}
      <details className="mt-3 text-xs text-dim"><summary>Identity & copy</summary><code className="mt-2 block break-all text-[10px]">{did}</code><button className="mt-2 text-cyan" onClick={async()=>{try{await navigator.clipboard.writeText(did);setCopied(true)}catch{setCopied(false)}}}>{copied?'Copied':'Copy DID'}</button></details>
      <Link to="/accounts/$did" params={{did}} onClick={close} className="mt-4 block border-t border-line pt-3 text-xs text-cyan">Open full profile →</Link>
    </div>,document.body)}
  </span>
}
