export type GraphNode = { id:string; label:string; weight:number; meta:string }
export type GraphEdge = { source:string; target:string; weight:number; types:string[] }
export const GRAPH_WIDTH=1000, GRAPH_HEIGHT=560
export function layoutGraph(nodes:GraphNode[],edges:GraphEdge[]) {
  const maximum=Math.max(1,...nodes.map(n=>Math.max(0,n.weight)))
  const points=nodes.map((node,i)=>{
    const angle=i*2.3999632297, distance=Math.sqrt((i+.5)/Math.max(1,nodes.length))*210
    return {...node,x:500+Math.cos(angle)*distance*1.7,y:280+Math.sin(angle)*distance,radius:5+7*Math.log1p(Math.max(0,node.weight))/Math.log1p(maximum)}
  })
  const indices=new Map(points.map((p,i)=>[p.id,i]))
  const links=edges.flatMap(e=>{const a=indices.get(e.source),b=indices.get(e.target);return a===undefined||b===undefined||a===b?[]:[{a,b,weight:e.weight}]})
  // Fixed iterations and deterministic seeding avoid perpetual animation/jitter.
  // Only the bounded visible graph is laid out; distance is not a social metric.
  for(let step=0;step<240;step++) {
    const force=points.map(p=>({x:(500-p.x)*.006,y:(280-p.y)*.006}))
    for(let i=0;i<points.length;i++)for(let j=i+1;j<points.length;j++) {
      const dx=points[j].x-points[i].x,dy=points[j].y-points[i].y,d=Math.max(.1,Math.hypot(dx,dy))
      const gap=points[i].radius+points[j].radius+14
      const strength=500/(d*d)+Math.max(0,gap-d)*.45
      force[i].x-=dx/d*strength;force[i].y-=dy/d*strength
      force[j].x+=dx/d*strength;force[j].y+=dy/d*strength
    }
    for(const {a,b,weight} of links) {
      const dx=points[b].x-points[a].x,dy=points[b].y-points[a].y,d=Math.max(1,Math.hypot(dx,dy))
      const strength=(d-90)*.004*Math.min(3,1+Math.log1p(Math.max(0,weight))*.15)
      force[a].x+=dx/d*strength;force[a].y+=dy/d*strength
      force[b].x-=dx/d*strength;force[b].y-=dy/d*strength
    }
    const cooling=1-step/300
    points.forEach((p,i)=>{p.x=Math.max(35,Math.min(965,p.x+Math.max(-8,Math.min(8,force[i].x))*cooling));p.y=Math.max(35,Math.min(525,p.y+Math.max(-8,Math.min(8,force[i].y))*cooling))})
  }
  return points
}
