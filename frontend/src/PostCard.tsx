import { Link } from '@tanstack/react-router'
import { ArrowUpRight, MessageCircle, Quote } from 'lucide-react'
import type { RecordItem } from './types'

export function PostCard({ post, name }: { post: RecordItem; name?: string }) {
  return <article className="panel p-5 transition hover:border-cyan/40 sm:p-6">
    <div className="mb-4 flex flex-wrap items-center justify-between gap-2 text-xs text-dim">
      <Link to="/accounts/$did" params={{ did: post.actor_did }} className="max-w-full truncate font-medium text-cyan">{name || post.actor_did}</Link>
      <time dateTime={new Date(post.timestamp_ms).toISOString()}>{new Date(post.timestamp_ms).toLocaleString()}</time>
    </div>
    {post.kind === 'reply' && <p className="mb-3 flex items-center gap-2 text-xs text-dim"><MessageCircle className="size-3" />Reply</p>}
    <Link to="/records/$uri" params={{ uri: post.at_uri }} className="block whitespace-pre-wrap break-words text-[15px] leading-7 text-white/90">{post.text || 'Open this post to view its content'}</Link>
    {post.quote_target && <Link to="/records/$uri" params={{ uri: post.quote_target }} className="mt-4 flex items-center gap-2 rounded-lg border border-line p-3 text-xs text-cyan"><Quote className="size-4" />View quoted post</Link>}
    <div className="mt-5 flex items-center justify-between border-t border-line pt-3"><span className="text-xs text-dim">{post.languages.join(' · ') || 'Language unspecified'}</span><Link to="/records/$uri" params={{ uri: post.at_uri }} className="flex items-center gap-1 text-xs text-cyan">Open conversation <ArrowUpRight className="size-3" /></Link></div>
  </article>
}
