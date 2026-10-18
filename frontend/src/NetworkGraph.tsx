import { useMemo, useRef, useState } from 'react'
import { layoutGraph, GRAPH_WIDTH, GRAPH_HEIGHT, type GraphNode, type GraphEdge } from './graphLayout'

export function NetworkGraph({nodes,edges,onOpen,label}:{nodes:GraphNode[];edges:GraphEdge[];onOpen?:(id:string)=>void;label:string}) {
  const [view,setView]=useState({x:0,y:0,zoom:1})
  const [selected,setSelected]=useState<string|null>(null)
  const [hovered,setHovered]=useState<string|null>(null)
  const [showIsolated,setShowIsolated]=useState(false)
  const drag=useRef<{x:number;y:number;startX:number;startY:number;moved:boolean}|null>(null)
  const moved=useRef(false)
  const connected=useMemo(()=>{const ids=new Set(nodes.map(n=>n.id));return new Set(edges.filter(e=>e.source!==e.target&&ids.has(e.source)&&ids.has(e.target)).flatMap(e=>[e.source,e.target]))},[nodes,edges])
  const points=useMemo(()=>layoutGraph(showIsolated||!connected.size?nodes:nodes.filter(n=>connected.has(n.id)),edges),[nodes,edges,connected,showIsolated])
  const lookup=new Map(points.map(p=>[p.id,p]))
  const visibleEdges=edges.filter(e=>e.source!==e.target&&lookup.has(e.source)&&lookup.has(e.target))
  const active=lookup.has(hovered||'')?hovered:lookup.has(selected||'')?selected:null
  const node=active?lookup.get(active):undefined
  const neighbors=new Set(visibleEdges.filter(e=>e.source===active||e.target===active).flatMap(e=>[e.source,e.target]))
  const zoom=(factor:number)=>setView(v=>({...v,zoom:Math.max(.5,Math.min(5,v.zoom*factor))}))
  return <div className="border border-line bg-ink">
    {connected.size<nodes.length && <div className="border-b border-line p-3 text-xs text-dim"><label className="flex items-center gap-2"><input type="checkbox" checked={showIsolated} onChange={e=>{setShowIsolated(e.target.checked);setView({x:0,y:0,zoom:1});setSelected(null);setHovered(null)}}/>Include accounts without visible connections ({nodes.length-connected.size})</label><p className="mt-2">Missing links can reflect the member-page and edge limits, not a lack of relationships. All members remain in the table below.</p></div>}
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line p-3 text-xs"><span className="text-dim">{points.length} accounts / groups · {visibleEdges.length} visible connections</span><div className="flex gap-2"><button className="border border-line px-3 py-1.5" onClick={()=>zoom(1.25)} aria-label="Zoom in">+</button><button className="border border-line px-3 py-1.5" onClick={()=>zoom(.8)} aria-label="Zoom out">−</button><button className="border border-line px-3 py-1.5" onClick={()=>{setView({x:0,y:0,zoom:1});setSelected(null);setHovered(null)}}>Reset view</button></div></div>
    <svg role="group" aria-label={label} viewBox={`0 0 ${GRAPH_WIDTH} ${GRAPH_HEIGHT}`} className="h-96 w-full touch-none sm:h-[480px]" onPointerDown={e=>{if(e.button!==0)return;const matrix=e.currentTarget.getScreenCTM();if(!matrix)return;const p=new DOMPoint(e.clientX,e.clientY).matrixTransform(matrix.inverse());drag.current={x:p.x,y:p.y,startX:view.x,startY:view.y,moved:false};moved.current=false;e.currentTarget.setPointerCapture(e.pointerId)}} onPointerMove={e=>{const d=drag.current,matrix=e.currentTarget.getScreenCTM();if(!d||!matrix)return;const p=new DOMPoint(e.clientX,e.clientY).matrixTransform(matrix.inverse());if(Math.hypot(p.x-d.x,p.y-d.y)>5)d.moved=true;moved.current=d.moved;setView(v=>({...v,x:d.startX+p.x-d.x,y:d.startY+p.y-d.y}))}} onPointerUp={()=>{drag.current=null}} onPointerCancel={()=>{drag.current=null;moved.current=true}}>
      <g transform={`translate(${500+view.x} ${280+view.y}) scale(${view.zoom}) translate(-500 -280)`}>
        {visibleEdges.map((edge,i)=>{const a=lookup.get(edge.source)!,b=lookup.get(edge.target)!,highlight=edge.source===active||edge.target===active;return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={highlight?'#e9b96b':'#777'} strokeOpacity={active?(highlight?.8:.04):.18} strokeWidth={highlight?1.8:1} vectorEffect="non-scaling-stroke"/>})}
        {points.map(p=><g key={p.id} role="button" tabIndex={0} aria-label={`${p.label}, ${p.meta}`} aria-pressed={selected===p.id} onPointerDown={e=>{e.stopPropagation();moved.current=false}} onClick={()=>{if(!moved.current)setSelected(p.id);moved.current=false}} onKeyDown={e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();setSelected(p.id)}}} onFocus={()=>setHovered(p.id)} onBlur={()=>setHovered(null)} onMouseEnter={()=>setHovered(p.id)} onMouseLeave={()=>setHovered(null)} className="cursor-pointer">
          <circle cx={p.x} cy={p.y} r={Math.max(16,p.radius)} fill="transparent"/>
          <circle data-graph-node="true" cx={p.x} cy={p.y} r={p.radius} fill={active===p.id?'#e9b96b':neighbors.has(p.id)?'#c4a0e5':'#a3d99c'} opacity={!active||neighbors.has(p.id)||active===p.id?1:.2} stroke={active===p.id?'#fff':'#090909'} strokeWidth="1.5"/>
          {active===p.id && <text x={p.x} y={p.y-p.radius-10} textAnchor="middle" fill="#eee" stroke="#090909" strokeWidth="3" paintOrder="stroke" fontSize="12" pointerEvents="none">{p.label.length>30?`${p.label.slice(0,28)}…`:p.label}</text>}
        </g>)}
      </g>
    </svg>
    <div className="flex min-h-24 flex-wrap items-center justify-between gap-3 border-t border-line p-4 text-xs" aria-live="polite"><div className="min-w-0 flex-1">{node?<><p className="break-all font-medium text-cyan">{node.label}</p><p className="mt-2 text-dim">{node.meta} · {Math.max(0,neighbors.size-1)} visible neighbours</p></>:<p className="text-dim">Select a node to highlight its connections. Drag the background to pan; use + / − to zoom. Positions are illustrative, not geographic or a measure of similarity.</p>}</div>{node&&onOpen&&<button onClick={()=>onOpen(node.id)} className="border border-cyan px-3 py-2 text-cyan">Open details →</button>}</div>
  </div>
}
