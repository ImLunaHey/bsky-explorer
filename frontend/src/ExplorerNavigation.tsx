import { Link } from '@tanstack/react-router'
import { useState } from 'react'
import { Network, Users, Compass, Music2, Boxes, Database, Radio, Search } from 'lucide-react'

export function ExplorerNavigation({view}:{view:string}) {
  const [open,setOpen] = useState(false)
  const items = [
    {to:'/network',label:'Network',icon:Network,active:view==='network'},
    {to:'/search',label:'People & search',icon:Users,active:['search','account'].includes(view)},
    {to:'/',label:'Discover',icon:Compass,active:['discover','explore','story'].includes(view)},
    {to:'/music',label:'Music',icon:Music2,active:['music','music-detail'].includes(view)},
    {to:'/apps',label:'Apps',icon:Boxes,active:['apps','app'].includes(view)},
    {to:'/collections',label:'Collections',icon:Database,active:view==='collections'},
    {to:'/firehose',label:'Live activity',icon:Radio,active:['firehose','firehose-images'].includes(view)},
    {to:'/advanced',label:'Research tools',icon:Search,active:['advanced','communities','community','anomalies','cluster'].includes(view)},
  ] as const
  return <nav aria-label="Main navigation" className="mb-5 border-b border-line">
    <button aria-expanded={open} aria-controls="explorer-links" onClick={()=>setOpen(!open)} className="flex w-full items-center justify-between px-3 py-2 text-sm lg:hidden"><span>{items.find(item=>item.active)?.label || 'ATProto Explorer'}</span><span className="text-cyan">{open?'Close menu ↑':'Explore menu ↓'}</span></button>
    <div id="explorer-links" className={`${open?'grid':'hidden'} grid-cols-2 sm:grid-cols-4 lg:flex lg:flex-wrap`}>{items.map(({to,label,icon:Icon,active})=><Link key={to} to={to} onClick={()=>setOpen(false)} search={to==='/search'?{q:'',type:'account',offset:0}:to==='/collections'?{collection:''}:undefined} aria-current={active?'page':undefined} className={`flex min-w-0 items-center gap-2 border-b-2 px-3 py-3 text-xs font-medium transition ${active?'border-white bg-white/5 text-white':'border-transparent text-dim hover:bg-white/5 hover:text-white'}`}><Icon className="size-3.5 shrink-0"/><span>{label}</span></Link>)}</div>
  </nav>
}
