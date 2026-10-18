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
  return <nav aria-label="Main navigation" className="mb-6 rounded-xl border border-line bg-panel/70 p-2 lg:fixed lg:inset-y-19 lg:left-0 lg:mb-0 lg:w-52 lg:overflow-y-auto lg:rounded-none lg:border-y-0 lg:border-l-0 lg:p-4">
    <p className="mb-3 hidden px-3 pt-3 text-[10px] uppercase tracking-widest text-dim lg:block">Explore ATProto</p>
    <button aria-expanded={open} aria-controls="explorer-links" onClick={()=>setOpen(!open)} className="flex w-full items-center justify-between px-3 py-2 text-sm lg:hidden"><span>{items.find(item=>item.active)?.label || 'ATProto Explorer'}</span><span className="text-cyan">{open?'Close menu ↑':'Explore menu ↓'}</span></button>
    <div id="explorer-links" className={`${open?'grid':'hidden'} grid-cols-2 gap-1 sm:grid-cols-4 lg:flex lg:flex-col`}>{items.map(({to,label,icon:Icon,active})=><Link key={to} to={to} onClick={()=>setOpen(false)} search={to==='/search'?{q:'',type:'account',offset:0}:to==='/collections'?{collection:''}:undefined} aria-current={active?'page':undefined} className={`flex min-w-0 items-center gap-2 rounded-lg px-3 py-2.5 text-xs font-medium transition lg:text-sm ${active?'bg-cyan/15 text-cyan':'text-dim hover:bg-white/5 hover:text-white'}`}><Icon className="size-4 shrink-0"/><span>{label}</span></Link>)}</div>
    <div className="mt-8 hidden space-y-3 border-t border-line px-3 pt-5 text-xs text-dim lg:block"><Link to="/coverage" className="block">Coverage & privacy</Link><Link to="/admin" className="block">Administration</Link><p className="pt-3 leading-5">Public network activity.<br/>Not a complete census.</p></div>
  </nav>
}
