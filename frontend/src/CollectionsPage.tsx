import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { api } from './api'
import type { CoverageResponse } from './types'
import { categories, collectionInfo, type Category } from './collectionEnrichment'
import { EnrichedRecordCard } from './EnrichedRecordCard'

export type RepositoryRecord = { at_uri: string; actor_did: string; collection: string; operation: string; timestamp_ms: number; record_json: string }

export function CollectionsPage({ collection }: { collection: string }) {
  const [filter, setFilter] = useState('')
  const [visibleCount, setVisibleCount] = useState(60)
  const [beyondBluesky, setBeyondBluesky] = useState(false)
  const [category, setCategory] = useState<Category>('All')
  const inventory = useQuery({ queryKey: ['coverage'], queryFn: () => api<CoverageResponse>('/api/coverage'), staleTime: 300_000 })
  const records = useQuery({ queryKey: ['collection-records', collection], queryFn: () => api<RepositoryRecord[]>(`/api/collection-records?collection=${encodeURIComponent(collection)}`), enabled: !!collection })
  const collections = inventory.data?.collections.filter(item => `${item.collection} ${collectionInfo(item.collection).label}`.toLowerCase().includes(filter.toLowerCase()) && (!beyondBluesky || !item.collection.startsWith('app.bsky.')) && (category === 'All' || collectionInfo(item.collection).category === category))
  return <div className="space-y-6">
    <header><p className="eyebrow">Repositories, not just social posts</p><h1 className="mt-2 text-3xl font-semibold tracking-tight">Explore ATProto collections</h1><p className="mt-3 max-w-3xl text-sm leading-7 text-dim">Every app defines collections of public records. Browse the collection identifiers observed by this collector, then inspect their records. Namespaces identify record schemas; they do not prove who operates an app.</p></header>
    {collection ? <>
      <Link to="/collections" search={{ collection: '' }} className="text-sm text-cyan">← All collections</Link>
      <section className="panel p-5"><p className="eyebrow">{collectionInfo(collection).label}</p><h2 className="mt-3 break-all font-mono text-lg text-cyan">{collection}</h2><p className="mt-3 text-sm text-dim">{collectionInfo(collection).description}</p><p className="mt-3 text-xs text-dim">Up to 30 latest retained record events from the last 24 hours. Imported timestamps and older records may fall outside this view.</p>{collectionInfo(collection).category === 'Music' && <Link to="/music" className="mt-4 inline-block text-sm text-cyan">Explore listening charts →</Link>}</section>
      {records.isPending && <p role="status" className="panel p-8 text-dim">Loading recent records…</p>}
      {records.isError && <p role="alert" className="panel p-8 text-dim">This collection couldn’t load within the request budget. <button onClick={() => void records.refetch()} className="text-cyan">Retry</button></p>}
      <div className="grid gap-4 md:grid-cols-2">{records.data?.map(record => <EnrichedRecordCard key={record.at_uri} record={record} />)}</div>
      {records.data?.length === 0 && <p className="panel p-8 text-sm text-dim">No record events retained for this collection in the last 24 hours.</p>}
    </> : <>
      <Link to="/music" className="panel block bg-gradient-to-r from-sky-500/15 to-indigo-500/10 p-6 hover:border-cyan/50"><p className="eyebrow">Listen across the atmosphere</p><h2 className="mt-3 text-2xl font-semibold">The network has a soundtrack.</h2><p className="mt-2 text-sm text-dim">Explore the most-played songs, artists and albums in observed listening records.</p><p className="mt-4 text-sm text-cyan">Open music charts →</p></Link>
      <div className="flex flex-wrap gap-2" aria-label="Collection categories">{categories.map(value => <button key={value} aria-pressed={category === value} onClick={() => { setCategory(value); setVisibleCount(60) }} className={`rounded-full border px-3 py-2 text-xs ${category === value ? 'border-cyan bg-cyan text-ink' : 'border-line text-dim'}`}>{value}</button>)}</div>
      <div className="flex flex-wrap items-center gap-4"><input aria-label="Filter collection identifiers" value={filter} onChange={e => { setFilter(e.target.value); setVisibleCount(60) }} placeholder="Filter by app, namespace or collection…" className="min-w-0 flex-1 rounded-xl border border-line bg-panel px-4 py-3 text-sm" /><label className="flex items-center gap-2 text-sm text-dim"><input type="checkbox" checked={beyondBluesky} onChange={e => { setBeyondBluesky(e.target.checked); setVisibleCount(60) }} />Beyond Bluesky</label></div>
      {inventory.isPending && <p role="status" className="panel p-8 text-dim">Loading collection inventory…</p>}
      {inventory.isError && <p role="alert" className="panel p-8 text-dim">The collection inventory is being refreshed. <button onClick={() => void inventory.refetch()} className="text-cyan">Try again</button></p>}
      <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-3">{collections?.slice(0,visibleCount).map(item => <Link key={item.collection} to="/collections" search={{ collection: item.collection }} className="panel p-5 transition hover:border-cyan/50"><p className="eyebrow">{collectionInfo(item.collection).category} · {item.namespace}</p><h2 className="mt-3 break-all font-mono text-sm text-cyan">{item.collection}</h2><p className="mt-3 text-xs leading-6 text-dim">{collectionInfo(item.collection).description}</p><p className="mt-5 text-xs text-dim">{item.records.toLocaleString()} retained events · {item.unique_accounts.toLocaleString()} observed accounts</p></Link>)}</div>
      {!!collections?.length && <div className="flex items-center justify-between gap-4 text-xs text-dim"><span>Showing {Math.min(visibleCount,collections.length)} of {collections.length} matching collections</span>{collections.length > visibleCount && <button onClick={() => setVisibleCount(value => value+60)} className="rounded-lg border border-line px-4 py-2 text-cyan">Show more collections</button>}</div>}
      {collections?.length === 0 && <p className="panel p-8 text-sm text-dim">No retained collections match these filters.</p>}
      <p className="text-xs leading-6 text-dim">Inventory is periodically refreshed and limited to the 500 most active retained collections. This is not a registry of all ATProto apps or a complete mirror of the network.</p>
    </>}
  </div>
}

export function GenericRecordPage({ uri }: { uri: string }) {
  const record = useQuery({ queryKey: ['repository-record', uri], queryFn: () => api<RepositoryRecord>(`/api/repository-record?uri=${encodeURIComponent(uri)}`) })
  const data = record.data
  let json = data?.record_json || ''
  try { json = JSON.stringify(JSON.parse(json), null, 2) } catch { /* preserve retained payload */ }
  return <section className="mx-auto max-w-5xl space-y-5"><header><p className="eyebrow">ATProto record</p><h1 className="mt-2 break-all font-mono text-xl text-cyan">{uri}</h1></header>
    {record.isPending && <p role="status" className="panel p-8 text-dim">Loading retained record…</p>}
    {record.isError && <p role="alert" className="panel p-8 text-dim">This record is unavailable or the lookup timed out. This viewer only shows retained data. <button onClick={() => void record.refetch()} className="text-cyan">Retry</button></p>}
    {data && <><EnrichedRecordCard record={data} /><div className="panel flex flex-wrap gap-5 p-5 text-sm"><Link to="/accounts/$did" params={{ did: data.actor_did }} className="break-all text-cyan">Explore repository →</Link><Link to="/collections" search={{ collection: data.collection }} className="break-all text-cyan">{data.collection} →</Link><span className="text-dim">{data.operation} · {new Date(data.timestamp_ms).toLocaleString()}</span></div>
      <section className="panel overflow-hidden"><h2 className="border-b border-line p-5 font-medium">Retained record data</h2><pre className="scrollbar overflow-auto p-5 text-xs leading-6 text-white/80">{json || (data.operation === 'delete' ? 'A deletion was observed. No record body accompanies this event.' : 'No record body was retained for this event.')}</pre></section><p className="text-xs leading-6 text-dim">Latest retained observation, not a live repository lookup. Compact archive records may omit blobs or fields; this is not guaranteed to be a complete original record.</p></>}
  </section>
}
