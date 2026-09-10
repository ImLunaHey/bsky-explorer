/* eslint-disable react-refresh/only-export-components */
import { useQueries } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from './api'
import type { Identity } from './types'

export const shortDid=(did:string)=>did.length>34?`${did.slice(0,20)}…${did.slice(-8)}`:did
export const didFromValue=(value:string)=>value.startsWith('at://')?value.slice(5).split('/')[0]:value.startsWith('did:')?value:undefined

export function useIdentityMap(values:string[]) {
  const dids=[...new Set(values.map(didFromValue).filter((did):did is string=>Boolean(did)))].sort()
  const batches=Array.from({length:Math.ceil(dids.length/100)},(_,index)=>dids.slice(index*100,(index+1)*100))
  const queries=useQueries({queries:batches.map(batch=>({queryKey:['identities',batch.join(',')],queryFn:()=>api<Identity[]>(`/api/identities?dids=${encodeURIComponent(batch.join(','))}`),staleTime:300_000,gcTime:3_600_000}))})
  return new Map(queries.flatMap(query=>query.data||[]).map(identity=>[identity.did,identity.handle]))
}

export function IdentityLink({did,handle,compact=false,className=''}:{did:string;handle?:string;compact?:boolean;className?:string}) {
  return <Link to="/accounts/$did" params={{did}} title={`${handle?`@${handle} · `:''}${did}`} className={`block min-w-0 text-left text-orange-300 hover:text-orange-200 ${className}`}><span className="block truncate font-medium">{handle?`@${handle}`:shortDid(did)}</span>{handle&&!compact&&<span className="block truncate font-mono text-[10px] text-white/35">{shortDid(did)}</span>}</Link>
}
