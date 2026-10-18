import { Link } from '@tanstack/react-router'
import { ArrowUpRight, MessageCircle, Quote } from 'lucide-react'
import type { RecordItem } from './types'
import { PostMedia } from './PostMedia'
import { safeWebUrl } from './collectionEnrichment'

export function PostCard({ post, name }: { post: RecordItem; name?: string }) {
  const links = [...new Set(post.urls.map(safeWebUrl).filter((url): url is string => Boolean(url)))]
  return <article className="panel p-5 transition hover:border-cyan/40 sm:p-6">
    <div className="mb-4 flex flex-wrap items-center justify-between gap-2 text-xs text-dim">
      <Link to="/accounts/$did" params={{ did: post.actor_did }} className="max-w-full truncate font-medium text-cyan">{name || post.actor_did}</Link>
      <time dateTime={new Date(post.timestamp_ms).toISOString()}>{new Date(post.timestamp_ms).toLocaleString()}</time>
    </div>
    {post.kind === 'reply' && <p className="mb-3 flex items-center gap-2 text-xs text-dim"><MessageCircle className="size-3" />Reply</p>}
    {post.text && <Link to="/records/$uri" params={{ uri: post.at_uri }} className="block whitespace-pre-wrap break-words text-[15px] leading-7 text-white/90">{post.text}</Link>}
    <PostMedia uri={post.at_uri}/>
    {!!links.length && <div className="mt-3 flex flex-wrap gap-2">{links.map(url=><a key={url} href={url} target="_blank" rel="noreferrer" className="max-w-full truncate border border-line px-3 py-2 text-xs text-cyan" title={url}>{new URL(url).hostname} ↗</a>)}</div>}
    {post.quote_target && <Link to="/records/$uri" params={{ uri: post.quote_target }} className="mt-4 flex items-center gap-2 rounded-lg border border-line p-3 text-xs text-cyan"><Quote className="size-4" />View quoted post</Link>}
    <div className="mt-5 flex items-center justify-between border-t border-line pt-3"><span className="text-xs text-dim">{post.languages.join(' · ') || 'Language unspecified'}</span><Link to="/records/$uri" params={{ uri: post.at_uri }} className="flex items-center gap-1 text-xs text-cyan">Open conversation <ArrowUpRight className="size-3" /></Link></div>
  </article>
}
