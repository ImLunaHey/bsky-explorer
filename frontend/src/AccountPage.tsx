import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { ArrowUpRight, Copy, UserRound } from 'lucide-react'
import { api } from './api'
import { AccountFollows } from './AccountFollows'
import { AccountLexicons } from './AccountLexicons'
import { PostCard } from './PostCard'
import type { AccountDetail, AccountEvent, AccountProfile, RecordItem } from './types'
import type { RepositoryRecord } from './CollectionsPage'
import { ContentCards } from './DiscoveryCards'
import { appFor, topicFor } from './discovery'

const count = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })
const tabs = ['Highlights', 'Writing', 'Listening', 'Posts', 'Connections'] as const
type Tab = typeof tabs[number]

export function AccountPage({ did }: { did: string }) {
  return <Profile key={did} did={did} />
}

function Profile({ did }: { did: string }) {
  const navigate = useNavigate()
  const [tab, setTab] = useState<Tab>('Highlights')
  const [technical, setTechnical] = useState(false)
  const [copied, setCopied] = useState(false)
  const [avatarFailed, setAvatarFailed] = useState(false)
  const [expandedBio, setExpandedBio] = useState(false)
  const profile = useQuery({ queryKey: ['account-profile', did], queryFn: () => api<AccountProfile>(`/api/accounts/${encodeURIComponent(did)}/profile`), staleTime: 300_000 })
  const account = useQuery({ queryKey: ['account', did], queryFn: () => api<AccountDetail>(`/api/accounts/${encodeURIComponent(did)}`), staleTime: 60_000 })
  const person = profile.data
  const handle = person?.handle || account.data?.current_identity.handle
  const name = person?.display_name || (handle ? `@${handle}` : 'A person on the open web')
  return <div className="mx-auto max-w-5xl space-y-6">
    <Link to="/search" search={{ q: "", type: "account", offset: 0 }} className="text-sm text-dim hover:text-cyan">← Explore people</Link>
    <section className="panel overflow-hidden">
      <div className="profile-cover h-28 sm:h-40" aria-hidden="true" />
      <div className="px-5 pb-6 sm:px-8">
        <div className="flex items-end justify-between gap-3">
          <div className="-mt-10 grid size-24 shrink-0 place-items-center overflow-hidden rounded-3xl border-4 border-panel bg-ink shadow-xl">
            {person?.avatar_url && !avatarFailed ? <img src={person.avatar_url} alt="" onError={() => setAvatarFailed(true)} referrerPolicy="no-referrer" className="size-full object-cover" /> : <UserRound className="size-10 text-cyan" />}
          </div>
          <a href={`https://bsky.app/profile/${encodeURIComponent(did)}`} target="_blank" rel="noreferrer" className="flex items-center gap-2 rounded-full border border-line px-4 py-2 text-xs hover:border-cyan">View on Bluesky <ArrowUpRight className="size-4" /></a>
        </div>
        <h1 className="mt-5 break-words text-3xl font-semibold tracking-tight">{name}</h1>
        <p className="mt-1 break-all text-sm text-cyan">{handle ? `@${handle}` : did}</p>
        {profile.isPending && <p className="mt-3 text-sm text-dim">Loading public profile…</p>}
        {profile.isError && <p className="mt-3 text-sm text-dim">No Bluesky profile available. This ATProto repository is still explorable.</p>}
        {person?.description && <><p className={`mt-4 max-w-2xl whitespace-pre-wrap break-words text-sm leading-7 text-white/85 ${expandedBio ? '' : 'line-clamp-4'}`}>{person.description}</p>{(person.description.length > 180 || person.description.split('\n').length > 4) && <button onClick={() => setExpandedBio(value => !value)} className="mt-2 text-xs text-cyan">{expandedBio ? 'Show less' : 'Read full bio'}</button>}</>}
        <div className="mt-5 flex flex-wrap gap-x-6 gap-y-2 text-sm text-dim">
          {([['followers', person?.followers_count], ['following', person?.follows_count], ['posts', person?.posts_count]] as const).map(([label, value]) => value != null && <span key={label}><b className="text-white">{count.format(value)}</b> {label}</span>)}
        </div>
        <details className="mt-5 text-xs text-dim"><summary className="cursor-pointer">Account identity & collection details</summary>
          <div className="mt-3 space-y-3"><div className="flex items-center gap-3"><code className="break-all">{did}</code><button aria-label="Copy DID" onClick={async () => { try { await navigator.clipboard.writeText(did); setCopied(true) } catch { setCopied(false) } }}><Copy className="size-4" />{copied && 'Copied'}</button></div>
          {account.data?.stored && <p>{account.data.stored.event_count.toLocaleString()} retained events · first seen {new Date(account.data.stored.first_seen_ms).toLocaleDateString()}</p>}
          {account.isError && <p>Archive statistics temporarily unavailable.</p>}
          <p>Profile counts are from Bluesky. Activity below covers this explorer’s retained data, not the account’s complete history.</p>
          {person?.labels.length ? <p>Labels: {person.labels.join(', ')}</p> : null}
          <Link to="/backfills" className="text-cyan">Manage collection →</Link></div>
        </details>
      </div>
      <div className="flex overflow-x-auto border-t border-line px-3" role="tablist" aria-label="Profile sections">
        {tabs.map(value => <button key={value} role="tab" aria-selected={tab === value} aria-controls="profile-panel" id={`profile-tab-${value}`} onClick={() => setTab(value)} className={`shrink-0 border-b-2 px-4 py-4 text-sm font-medium transition ${tab === value ? 'border-cyan text-cyan' : 'border-transparent text-dim hover:text-white'}`}>{value}</button>)}
      </div>
    </section>
    <section id="profile-panel" role="tabpanel" aria-labelledby={`profile-tab-${tab}`}>
      {tab === 'Posts' && <Posts did={did} name={name} />}
      {(['Highlights','Writing','Listening'] as string[]).includes(tab) && <CrossAppContent did={did} section={tab} />}
      {tab === 'Connections' && <div className="panel p-5"><AccountFollows did={did} onAccount={accountDid => void navigate({ to: '/accounts/$did', params: { did: accountDid } })} /></div>}
    </section>
    <details className="panel p-5 text-sm" onToggle={event=>setTechnical(event.currentTarget.open)}><summary className="text-dim">Technical account details</summary>{technical && <div className="mt-5 space-y-6"><AccountLexicons did={did}/><Activity did={did}/><Investigations did={did}/></div>}</details>
  </div>
}

function CrossAppContent({did,section}:{did:string;section:Tab}) {
  const query = useQuery({queryKey:['account-content',did],queryFn:()=>api<RepositoryRecord[]>(`/api/accounts/${encodeURIComponent(did)}/content`),staleTime:60_000})
  const records = query.data || []
  const seenApps = [...new Set(records.map(record=>appFor(record.collection)).filter(Boolean))]
  const selected = records.filter(record=>section === 'Writing' ? topicFor(record.collection)==='writing' : section==='Listening' ? topicFor(record.collection)==='music' : true)
  // Highlights are deliberately varied: up to three examples from each interest.
  const counts = new Map<string,number>()
  const shown = selected.filter(record=>{const category=topicFor(record.collection)||'other';const count=counts.get(category)||0;counts.set(category,count+1);return section !== 'Highlights' || count < 3}).slice(0,section==='Highlights'?12:24)
  return <div className="space-y-5"><header><h2 className="text-xl font-semibold">{section==='Highlights'?'A life across apps':section==='Writing'?'Writing & ideas':'Recently shared listening'}</h2><p className="mt-2 text-sm text-dim">{section==='Highlights'?'Some of what this person creates, enjoys and shares.':'Selected public records from this person’s activity.'} This is what we’ve retained, not their full history.</p></header>{section==='Highlights' && seenApps.length>0 && <div className="flex flex-wrap gap-2">{seenApps.map(app=>app && <Link key={app.id} to="/apps/$appId" params={{appId:app.id}} className="topic-chip">{app.name} →</Link>)}</div>}{query.isPending && <Loading/>}{query.isError && <Failure retry={()=>void query.refetch()}/>}<ContentCards records={shown}/>{query.isSuccess && !shown.length && <div className="panel p-6 text-sm leading-7 text-dim">We haven’t collected {section==='Writing'?'writing':section==='Listening'?'listening':'cross-app highlights'} for this person yet. That isn’t evidence they don’t use other apps. Check their posts or explore the apps directly.<div className="mt-3"><Link to="/apps" className="text-cyan">Explore apps →</Link></div></div>}</div>
}

function Posts({ did, name }: { did: string; name: string }) {
  const [replies, setReplies] = useState(true)
  const posts = useQuery({ queryKey: ['account-posts', did], queryFn: () => api<RecordItem[]>(`/api/accounts/${encodeURIComponent(did)}/posts`) })
  const visible = posts.data?.filter(post => replies || post.kind !== 'reply')
  return <div className="space-y-4"><div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-lg font-semibold">Recent posts</h2><p className="mt-1 text-xs text-dim">Latest 50 retained posts, replies and quotes. This is not a complete timeline.</p></div><label className="flex items-center gap-2 text-xs text-dim"><input type="checkbox" checked={replies} onChange={e => setReplies(e.target.checked)} />Include replies</label></div>
    {posts.isPending && <Loading />}{posts.isError && <Failure retry={() => void posts.refetch()} />}
    {visible?.map(post => <PostCard key={post.at_uri} post={post} name={name} />)}
    {!posts.isPending && !posts.isError && !visible?.length && <Empty text="No retained posts in this view. Try including replies or open this account on Bluesky." />}
  </div>
}

function Activity({ did }: { did: string }) {
  const events = useQuery({ queryKey: ['account-events', did], queryFn: () => api<AccountEvent[]>(`/api/accounts/${encodeURIComponent(did)}/events`) })
  return <div className="panel p-5"><h2 className="text-lg font-semibold">Latest observed activity</h2><p className="mt-1 text-xs text-dim">Up to 100 retained events. Event times may include imported history.</p>
    {events.isPending && <Loading />}{events.isError && <Failure retry={() => void events.refetch()} />}
    {events.data?.map(event => <div key={event.event_id} className="flex gap-4 border-b border-line py-4 last:border-0"><span className="w-16 shrink-0 text-xs capitalize text-cyan">{event.kind}</span><div className="min-w-0 flex-1"><p className="whitespace-pre-wrap break-words text-sm">{event.text || 'Public repository activity'}</p>{(event.target || event.at_uri).includes('/app.bsky.feed.post/') && <Link to="/records/$uri" params={{ uri: event.target || event.at_uri }} className="mt-1 inline-block text-xs text-cyan">Open post →</Link>}<p className="mt-2 text-xs text-dim">{new Date(event.time_us / 1000).toLocaleString()}</p></div></div>)}
    {events.data?.length === 0 && <Empty text="No activity has been retained for this account yet." />}
  </div>
}

function Investigations({ did }: { did: string }) {
  const query = useQuery({ queryKey: ['account-investigations', did], queryFn: () => api<string[]>(`/api/accounts/${encodeURIComponent(did)}/investigations`) })
  return <div className="panel p-5"><h2 className="text-lg font-semibold">Coordination investigations</h2><p className="mt-2 text-sm text-dim">Observed patterns are leads for investigation, not proof of automation or wrongdoing.</p>{query.isPending && <Loading />}{query.isError && <Failure retry={() => void query.refetch()} />}{query.data?.map(id => <Link key={id} to="/clusters/$clusterId" params={{ clusterId: id }} className="mt-3 block break-all text-sm text-cyan">{id} →</Link>)}{query.data?.length === 0 && <Empty text="No indexed investigations for this account." />}</div>
}

function Loading() { return <div role="status" className="panel animate-pulse p-8 text-sm text-dim">Loading this section…</div> }
function Failure({ retry }: { retry: () => void }) { return <div role="alert" className="panel p-5 text-sm text-dim">This section couldn’t load. Other sections are still available. <button onClick={retry} className="ml-2 text-cyan">Try again</button></div> }
function Empty({ text }: { text: string }) { return <p className="py-12 text-center text-sm leading-6 text-dim">{text}</p> }
