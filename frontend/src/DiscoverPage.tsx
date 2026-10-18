import { CardSkeletons } from './Skeletons'
import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ArrowUpRight, Compass, Music2, BookOpen, Film, CalendarDays } from 'lucide-react'
import { api } from './api'
import { apps, topics, type Discovery, type Music } from './discovery'
import { ContentCards, Freshness, RetryNotice, SongCards } from './DiscoveryCards'

export function DiscoverPage({ topic }: { topic?: string }) {
  const exploring = topic !== undefined
  const selected = topics.find(item => item.id === topic)
  const [visible, setVisible] = useState(12)
  const feed = useQuery({ queryKey:['discovery'],queryFn:()=>api<Discovery>('/api/discover'),staleTime:60_000 })
  const music = useQuery({ queryKey:['music','24h'],queryFn:()=>api<Music>('/api/music?window=24h'),staleTime:60_000,enabled:!topic || topic === 'music' })
  const sections = feed.data?.sections.filter(section => (!topic || section.category === topic) && section.records.length >= (exploring ? 1 : 3)) || []
  const songs = music.data?.tracks_by_listeners || []
  return <div className="space-y-10 pb-8">
    <header className="discovery-hero relative overflow-hidden rounded-3xl border border-line px-6 py-10 sm:px-10 sm:py-14">
      <p className="eyebrow flex items-center gap-2"><Compass className="size-4" />{exploring ? 'Follow your curiosity' : 'One identity. A world to explore.'}</p>
      <h1 className="mt-5 max-w-3xl text-4xl font-semibold leading-[1.08] tracking-tight sm:text-6xl">{selected ? selected.label : exploring ? 'Find your kind of interesting.' : <>There’s more here<br />than a timeline.</>}</h1>
      <p className="mt-5 max-w-xl text-base leading-7 text-dim">{selected ? selected.description : 'Discover what people are creating, sharing and listening to across the open social web.'}</p>
      {!exploring && <div className="mt-7 flex flex-wrap gap-3"><Link to="/explore" search={{topic:''}} className="rounded-full bg-cyan px-5 py-3 text-sm font-semibold text-ink">Find something new</Link><Link to="/apps" className="rounded-full border border-line px-5 py-3 text-sm">Meet the apps ↗</Link></div>}
    </header>
    <nav aria-label="Explore interests" className="flex flex-wrap gap-2">{exploring && <Link to="/explore" search={{topic:''}} className={`topic-chip ${!topic ? 'topic-active' : ''}`}>Everything</Link>}{topics.map(item => <Link key={item.id} to="/explore" search={{topic:item.id}} onClick={()=>setVisible(12)} className={`topic-chip ${topic === item.id ? 'topic-active' : ''}`}>{item.label}</Link>)}</nav>
    {(!topic || topic === 'music') && <section className="space-y-4"><div className="flex flex-wrap items-end justify-between gap-3"><div><p className="eyebrow flex items-center gap-2"><Music2 className="size-4" />The soundtrack today</p><h2 className="mt-2 text-2xl font-semibold tracking-tight">Songs bringing people together</h2><p className="mt-2 text-sm text-dim">Ranked by distinct listeners in our observed plays, not repeat listens.</p></div><Link to="/music" className="text-sm text-cyan">All music charts →</Link></div>{music.isPending && <CardSkeletons compact count={6}/>}{music.isError && <RetryNotice retry={()=>void music.refetch()} />}{songs.length > 0 && <SongCards rows={songs.slice(0,exploring ? visible : 6)} />}{music.isSuccess && songs.length === 0 && <p className="panel p-6 text-sm text-dim">Not enough listening data yet. <Link to="/music" className="text-cyan">Try the seven-day charts →</Link></p>}{music.data && <Freshness at={music.data.generated_at} />}</section>}
    {feed.isPending && (!topic || topic !== 'music') && <CardSkeletons/>}
    {feed.isError && (!topic || topic !== 'music') && <RetryNotice retry={()=>void feed.refetch()} />}
    {sections.map(section => { const name = topics.find(item => item.id === section.category); const Icon = section.category === 'writing' ? BookOpen : section.category === 'events' ? CalendarDays : Film; return <section key={section.category} className="space-y-4"><div className="flex flex-wrap items-end justify-between gap-3"><div><p className="eyebrow flex items-center gap-2"><Icon className="size-4" />{name?.label}</p><h2 className="mt-2 text-2xl font-semibold tracking-tight">{({writing:'Something worth reading',films:'On people’s screens',events:'Something to look forward to',bookmarks:'Worth saving',streams:'Recently on air',games:'Little wins, shared'} as Record<string,string>)[section.category]}</h2><p className="mt-2 text-sm text-dim">{name?.description}{section.category === 'streams' && ' Announcements, not verified live status.'}</p></div>{!topic && <Link to="/explore" search={{topic:section.category}} className="text-sm text-cyan">Explore {name?.label.toLowerCase()} →</Link>}</div><ContentCards records={section.records.slice(0,exploring ? visible : 3)} />{exploring && section.records.length > visible && <button className="topic-chip" onClick={()=>setVisible(v=>v+12)}>Show more</button>}</section>})}
    {exploring && topic && topic !== 'music' && feed.isSuccess && !sections.length && <section className="panel p-8"><h2 className="text-xl font-medium">A quiet corner—for now.</h2><p className="mt-3 text-sm text-dim">We don’t have a useful recent selection here yet. This doesn’t mean there’s no activity across the network.</p><Link to="/apps" className="mt-5 inline-block text-cyan">Find an app to explore directly →</Link></section>}
    {!topic && <section className="space-y-4"><div><p className="eyebrow">More than one app</p><h2 className="mt-2 text-2xl font-semibold">Take your curiosity somewhere new.</h2></div><div className="grid gap-4 sm:grid-cols-3">{[apps[2],apps[3],apps[5]].map(app=><Link key={app.id} to="/apps/$appId" params={{appId:app.id}} className="discovery-card p-6"><span className="grid size-12 place-items-center rounded-2xl bg-cyan/10 text-xl font-semibold text-cyan">{app.mark}</span><h3 className="mt-4 flex items-center justify-between text-lg font-semibold">{app.name}<ArrowUpRight className="size-4" /></h3><p className="mt-2 text-sm leading-6 text-dim">{app.description}</p></Link>)}</div></section>}
    {feed.data && topic !== 'music' && <Freshness at={feed.data.generated_at} />}
    <details className="text-xs leading-6 text-dim"><summary className="cursor-pointer">What am I looking at?</summary><p className="mt-3 max-w-3xl">ATProto lets different apps share a public network and portable identities. This explorer brings some of that activity together. These are selections from records we’ve observed, not a complete network index or editorial endorsements. Writing and links are deduplicated where source URLs match, with up to three items per author. Event dates come from their organizers. <Link to="/coverage" className="text-cyan">Read about coverage →</Link></p></details>
  </div>
}
