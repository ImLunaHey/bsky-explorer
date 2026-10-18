import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'
const code=ts.transpileModule(readFileSync(new URL('./graphLayout.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText
const module={exports:{}}
new Function('module','exports',code)(module,module.exports)
const {layoutGraph}=module.exports
test('100 heavily weighted nodes stay in bounds, small, separated and deterministic',()=>{
 const nodes=Array.from({length:100},(_,i)=>({id:String(i),label:String(i),weight:1e6,meta:''}))
 const edges=nodes.slice(1).map(n=>({source:'0',target:n.id,weight:1000,types:[]}))
 const points=layoutGraph(nodes,edges)
 assert.deepEqual(points,layoutGraph(nodes,edges))
 for(const p of points){assert.ok(p.radius<=12);assert.ok(p.x-p.radius>=0&&p.x+p.radius<=1000);assert.ok(p.y-p.radius>=0&&p.y+p.radius<=560)}
 for(let i=0;i<points.length;i++)for(let j=i+1;j<points.length;j++)assert.ok(Math.hypot(points[i].x-points[j].x,points[i].y-points[j].y)>=points[i].radius+points[j].radius,'nodes overlap')
})
test('empty, single and off-page edges are safe',()=>{
 assert.deepEqual(layoutGraph([],[]),[])
 const node={id:'one',label:'One',weight:0,meta:''}
 const result=layoutGraph([node],[{source:'missing',target:'one',weight:99,types:[]}])
 assert.equal(result.length,1);assert.ok(Number.isFinite(result[0].x));assert.equal(result[0].radius,5)
})
