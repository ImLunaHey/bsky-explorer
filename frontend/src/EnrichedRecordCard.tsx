import { Link } from '@tanstack/react-router'
import { enrichRecord } from './collectionEnrichment'
import type { RepositoryRecord } from './CollectionsPage'

export function EnrichedRecordCard({ record }: { record: RepositoryRecord }) {
  const preview = enrichRecord(record.collection, record.record_json, record.operation)
  const date = preview.date && Number.isFinite(Date.parse(preview.date)) ? new Date(preview.date).toLocaleString() : null
  return <article className="panel min-w-0 p-5">
    <div className="flex flex-wrap justify-between gap-2 text-xs text-dim"><span className="uppercase text-cyan">{record.operation}</span><span>Observed {new Date(record.timestamp_ms).toLocaleString()}</span></div>
    <Link to="/records/$uri" params={{ uri: record.at_uri }} className="mt-4 block break-words text-lg font-semibold text-white hover:text-cyan">{preview.title}</Link>
    <p className="mt-1 break-words text-sm text-cyan">{preview.subtitle}</p>
    {date && <p className="mt-2 text-xs text-dim">Record date: {date}</p>}
    {preview.body && <p className="mt-3 line-clamp-4 whitespace-pre-wrap break-words text-sm leading-6 text-dim">{preview.body}</p>}
    <div className="mt-3 flex flex-wrap gap-2">{preview.tags.map((tag, i) => <span key={`${tag}-${i}`} className="rounded-full border border-line px-2 py-1 text-[11px] text-dim">{tag}</span>)}</div>
    <div className="mt-5 flex flex-wrap gap-4 text-xs"><Link to="/accounts/$did" params={{ did: record.actor_did }} className="max-w-full truncate text-dim">{record.actor_did}</Link>{preview.url && <a href={preview.url} target="_blank" rel="noopener noreferrer" className="text-cyan">Open source ↗</a>}<Link to="/records/$uri" params={{ uri: record.at_uri }} className="text-cyan">Inspect record →</Link></div>
  </article>
}
