import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import ts from 'typescript'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

const source=readFileSync(new URL('./Skeletons.tsx',import.meta.url),'utf8')
const code=ts.transpileModule(source,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS}}).outputText
const module={exports:{}}
new Function('require','module','exports',code)(createRequire(import.meta.url),module,module.exports)
const render=(name,props={})=>renderToStaticMarkup(React.createElement(module.exports[name],props))
test('skeletons announce loading without exposing decorative placeholders or fake controls',()=>{
 for(const name of ['CardSkeletons','ListSkeleton','AnalyticsSkeleton','ProfileHeaderSkeleton','DetailSkeleton']) {
  const html=render(name)
  assert.match(html,/role="status"/)
  assert.match(html,/aria-busy="true"/)
  assert.match(html,/aria-hidden="true"/)
  assert.doesNotMatch(html,/<(?:button|input|a)\b/)
 }
})
test('route skeletons reserve distinct profile, chart, music and content structures',()=>{
 assert.match(render('PageSkeleton',{view:'account'}),/Loading profile/)
 assert.match(render('PageSkeleton',{view:'network'}),/skeleton-chart/)
 assert.match(render('PageSkeleton',{view:'music'}),/Loading music/)
 assert.match(render('PageSkeleton',{view:'discover'}),/Loading discoveries/)
 assert.equal((render('CardSkeletons',{count:6}).match(/discovery-card/g)||[]).length,6)
})
test('navigation stays outside the page loading boundary and motion can be disabled',()=>{
 const app=readFileSync(new URL('./App.tsx',import.meta.url),'utf8')
 assert.ok(app.indexOf('<ExplorerNavigation')<app.indexOf('<Suspense'))
 const css=readFileSync(new URL('./index.css',import.meta.url),'utf8')
 assert.match(css,/@media \(prefers-reduced-motion: reduce\) \{ \.skeleton-block \{ animation: none;/)
})
