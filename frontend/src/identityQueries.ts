import { queryOptions } from '@tanstack/react-query'
import { api } from './api'
import type { Identity } from './types'

// Coalesce per-DID query misses: 100 visible authors should not mean 100 HTTP calls.
export function createIdentityBatcher(fetchBatch: (dids: string[]) => Promise<Identity[]>) {
  const inFlight = new Map<string, Promise<Identity>>()
  let pending: { did: string; resolve: (value: Identity) => void; reject: (error: unknown) => void }[] = []
  let scheduled = false
  function flush() {
    const entries = pending
    pending = []
    scheduled = false
    for (let i = 0; i < entries.length; i += 100) {
      const batch = entries.slice(i, i + 100)
      void Promise.resolve().then(() => fetchBatch(batch.map(entry => entry.did))).then(records => {
        const byDid = new Map(records.map(record => [record.did, record]))
        for (const entry of batch) {
          inFlight.delete(entry.did)
          const record = byDid.get(entry.did)
          if (record) entry.resolve(record)
          else entry.reject(new Error('Identity response omitted an account'))
        }
      }).catch(error => {
        for (const entry of batch) { inFlight.delete(entry.did); entry.reject(error) }
      })
    }
  }
  return (did: string) => {
    const existing = inFlight.get(did)
    if (existing) return existing
    const promise = new Promise<Identity>((resolve, reject) => { pending.push({ did, resolve, reject }) })
    inFlight.set(did, promise)
    if (!scheduled) { scheduled = true; setTimeout(flush, 10) }
    return promise
  }
}

export const fetchIdentity = createIdentityBatcher(dids => api<Identity[]>(`/api/identities?dids=${encodeURIComponent(dids.join(','))}`))
export const identityQueryKey = (did: string) => ['identity', did] as const
export const needsIdentityLookup = (identity?: Identity) => !identity?.handle && (!identity || identity.source === 'unresolved')

export function identityQueryOptions(did: string, handle?: string) {
  return queryOptions({
    queryKey: identityQueryKey(did),
    queryFn: () => fetchIdentity(did),
    enabled: did.startsWith('did:'),
    initialData: handle ? { did, handle, observed_at: '', source: 'provided' } : undefined,
    staleTime: Infinity,
    // Session cache: navigating/remounting/focusing does not refetch known DIDs.
    gcTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: 1,
    refetchInterval: query => needsIdentityLookup(query.state.data)
      ? (query.state.error || query.state.dataUpdateCount > 15 ? 10_000 : 2_000)
      : false,
    refetchIntervalInBackground: false,
  })
}
