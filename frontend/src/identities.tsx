/* eslint-disable react-refresh/only-export-components */
import { useEffect, useMemo } from 'react'
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { identityQueryKey, identityQueryOptions, needsIdentityLookup } from './identityQueries'

export const didFromValue = (value: string) => value.startsWith('at://')
  ? value.slice(5).split('/')[0] : value.startsWith('did:') ? value : undefined

export function useIdentity(did: string, handle?: string) {
  const client = useQueryClient()
  const query = useQuery(identityQueryOptions(did, handle))
  // Search/profile results can supply a handle after another view cached a miss.
  useEffect(() => {
    if (handle && !client.getQueryData<{ handle?: string | null }>(identityQueryKey(did))?.handle) {
      client.setQueryData(identityQueryKey(did), { did, handle, observed_at: '', source: 'provided' })
    }
  }, [client, did, handle])
  return query
}

// Graphs, filtering and external URLs need strings rather than elements; they
// use exactly the same per-DID entries as the components below.
export function useIdentityMap(values: string[]) {
  const dids = [...new Set(values.map(didFromValue).filter((did): did is string => Boolean(did)))].sort()
  const queries = useQueries({ queries: dids.map(did => identityQueryOptions(did)) })
  const identities = queries.flatMap(query => query.data ? [query.data] : [])
  const signature = identities.map(identity => `${identity.did}\0${identity.handle || ''}`).join('\0')
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => new Map(identities.map(identity => [identity.did, identity.handle || undefined])), [signature])
}

type IdentityProps = { did: string; handle?: string; compact?: boolean; inline?: boolean; className?: string }

// Non-interactive variant for existing links, buttons, cards and table rows.
export function IdentityName({ did, handle: supplied, compact = false, inline = false, className = '' }: IdentityProps) {
  const query = useIdentity(did, supplied)
  const handle = query.data?.handle || supplied
  const status = query.isError ? 'Handle unavailable' : needsIdentityLookup(query.data) ? 'resolving…' : 'No handle available'
  return <span className={className} title={handle ? `@${handle} · ${did}` : did}>
    <span className={inline ? '' : 'block break-all font-medium'}>{handle ? `@${handle}` : did}</span>
    {!inline && (handle
      ? <span className={`block break-all font-mono text-white/35 ${compact ? 'text-[9px]' : 'text-[10px]'}`}>{did}</span>
      : <span className="block text-[9px] text-dim">{status}</span>)}
  </span>
}

export function IdentityLink({ did, handle, compact, inline, className = '', onSelect }: IdentityProps & { onSelect?: (did: string) => void }) {
  const content = <IdentityName did={did} handle={handle} compact={compact} inline={inline} />
  const styles = `${inline ? 'inline' : 'block'} min-w-0 text-left text-cyan hover:text-white ${className}`
  return onSelect
    ? <button type="button" onClick={() => onSelect(did)} className={styles}>{content}</button>
    : <Link to="/accounts/$did" params={{ did }} className={styles}>{content}</Link>
}
