import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import ts from 'typescript'
function load(file) {
 const code=ts.transpileModule(readFileSync(new URL(file,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText
 const module={exports:{}};new Function('module','exports',code)(module,module.exports);return module.exports
}
const {profileSearch,profileTab,profileDays}=load('./profileLocation.ts')
test('profile URLs validate tabs, time ranges and reply filtering',()=>{
 assert.equal(profileTab('posts'),'Posts');assert.equal(profileTab('audience'),'Audience')
 assert.equal(profileTab('unknown'),'Overview')
 assert.deepEqual(profileSearch({time:'7d',replies:'false'}),{time:'7d',replies:false})
 assert.deepEqual(profileSearch({time:'invalid',replies:'true'}),{time:undefined,replies:undefined})
 assert.deepEqual(['24h','7d','30d',undefined].map(profileDays),[1,7,30,1])
 assert.equal(profileSearch({posts:'media'}).posts,'media')
 assert.equal(profileSearch({posts:'invalid'}).posts,undefined)
})
const {eventAction,eventTarget}=load('./eventPresentation.ts')
test('firehose translates actions and resolves target accounts without raw URI prose',()=>{
 assert.equal(eventAction({kind:'follow',collection:''}),'Followed')
 assert.equal(eventAction({kind:'reply',collection:''}),'Replied to a post')
 assert.equal(eventTarget({target:'at://did:plc:target/app.bsky.feed.post/one'}),'did:plc:target')
 assert.equal(eventTarget({target_did:'did:web:example.org',target:'at://did:plc:old/post/one'}),'did:web:example.org')
 assert.equal(eventTarget({target:'https://example.com'}),undefined)
})
const {createMediaBatcher}=load('./postMedia.ts')
test('attachment metadata batches cap at 25 and preserve individual post results',async()=>{
 const calls=[]
 const fetch=createMediaBatcher(async uris=>{calls.push(uris);return {items:uris.map(uri=>({uri,media:[],found:uri!=='missing'}))}})
 const results=await Promise.all(Array.from({length:51},(_,i)=>fetch(i===0?'missing':String(i))))
 assert.deepEqual(calls.map(batch=>batch.length),[25,25,1]);assert.equal(results[0].found,false);assert.equal(results[1].found,true)
})
test('failed and incomplete media responses reject rather than hang',async()=>{
 const missing=createMediaBatcher(async()=>({items:[]}))
 await assert.rejects(missing('one'),/omitted/)
 const failed=createMediaBatcher(async()=>{throw Error('offline')})
 await assert.rejects(failed('one'),/offline/)
})
