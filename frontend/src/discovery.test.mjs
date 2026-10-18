import test from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import ts from 'typescript'
// Transpile the dependency-free presentation module, resolving its single runtime
// import explicitly for Node (Vite resolves extensionless TS imports in the app).
const source = (await readFile(new URL('./discovery.ts',import.meta.url),'utf8')).replace("'./collectionEnrichment'",JSON.stringify(new URL('./collectionEnrichment.ts',import.meta.url).href))
const {presentRecord, appFor, topicFor} = await import(`data:text/javascript;base64,${Buffer.from(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText).toString('base64')}`)
const record=(collection,value)=>({collection,record_json:JSON.stringify(value),at_uri:'at://did:plc:test/com.whtwnd.blog.entry/one',actor_did:'did:plc:test',operation:'create',timestamp_ms:0})
test('film actions distinguish watching from saving and unknown lists',()=>{
  assert.equal(presentRecord(record('social.popfeed.feed.listItem',{listType:'watched_movies'})).action,'Watched')
  assert.equal(presentRecord(record('social.popfeed.feed.listItem',{listType:'watchlist_movies'})).action,'Added to watchlist')
  assert.equal(presentRecord(record('social.popfeed.feed.listItem',{listType:'something_unknown'})).action,'Added to a film list')
})
test('readable writing avoids raw publication identifiers and preserves a source destination',()=>{
  assert.equal(presentRecord(record('site.standard.document',{title:'Article',site:'at://did:plc:test/site.standard.publication/self'})).subtitle,'Standard.site')
  const whitewind=presentRecord(record('com.whtwnd.blog.entry',{title:'Article'}))
  assert.equal(whitewind.url,'https://whtwnd.com/did%3Aplc%3Atest/one')
  assert.equal(whitewind.topic,'writing')
})
test('app recognition uses exact namespace prefixes and unknown records remain explorable',()=>{
  assert.equal(appFor('is.currents.feed.save').id,'currents')
  assert.equal(appFor('is.currentsFake.feed.save'),undefined)
  assert.equal(topicFor('unrecognized.app.record'),undefined)
  assert.equal(presentRecord(record('unrecognized.app.record',{title:'A thing'})).title,'A thing')
})
