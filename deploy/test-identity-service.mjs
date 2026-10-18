// Integration test against a disposable ClickHouse on this fixed test port.
// Never points at the production database. Run cargo build --bin identity_service first.
import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
const ch='http://127.0.0.1:18124';
const origin='http://127.0.0.1:13003';
async function sql(query) { const r=await fetch(ch,{method:'POST',body:query});const text=await r.text();assert.ok(r.ok,text);return text; }
const root=await mkdtemp(join(tmpdir(),'identity-service-test-'));
let child;
let logs='';
async function until(check) { for(let i=0;i<100;i++) { if(await check().catch(()=>false))return;await new Promise(r=>setTimeout(r,100)); }throw Error(`Timed out\n${logs}`); }
async function start() {
 child=spawn('./target/debug/identity_service',[],{env:{...process.env,CLICKHOUSE_URL:ch,CLICKHOUSE_DATABASE:'observatory',CLICKHOUSE_USER:'default',CLICKHOUSE_PASSWORD:'',IDENTITY_QUEUE_DIR:root,LISTEN_ADDR:'127.0.0.1:13003',RUST_LOG:'warn'},stdio:['ignore','pipe','pipe']});
 child.stdout.on('data',d=>logs+=d);child.stderr.on('data',d=>logs+=d);
 await until(async()=> (await fetch(`${origin}/readyz`)).ok);
}
async function stop(){ if(child?.exitCode===null){ const done=new Promise(r=>child.once('exit',r));child.kill('SIGTERM');await done; }child=undefined; }
async function resolve(dids) { const r=await fetch(`${origin}/resolve`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(dids)});assert.equal(r.status,200);return r.json(); }
try {
 await sql('DROP DATABASE IF EXISTS observatory'); // fixed disposable test server only
 await sql(await readFile('migrations/001_init.sql','utf8').then(s=>s.split(';')[0]));
 for(const statement of (await readFile('migrations/002_identities.sql','utf8')).split(';'))if(statement.trim())await sql(statement);
 await sql('CREATE TABLE IF NOT EXISTS observatory.account_history_events(account_did String) ENGINE=MergeTree ORDER BY account_did');
 const rows=Array.from({length:100},(_,i)=>({did:`did:plc:fixture${String(i).padStart(3,'0')}`,handle:`fixture${i}.test`,observed_at:'2020-01-01 00:00:00.000',source:'jetstream'}));
 await sql(`INSERT INTO observatory.identities FORMAT JSONEachRow\n${rows.map(r=>JSON.stringify(r)).join('\n')}`);
 await start();
 await until(async()=> (await (await fetch(`${origin}/stats`)).json()).bootstrap_complete);
 const dids=rows.map(r=>r.did).reverse();
 const startTime=performance.now();
 const records=await resolve(dids);
 assert.deepEqual(records.map(r=>r.did),dids);assert.equal(records[0].handle,'fixture99.test');
 console.log(`100 cached identities: ${Math.round(performance.now()-startTime)} ms`);
 const renamed={...rows[0],handle:'renamed.test',observed_at:'2026-10-18 23:00:00.000'};
 await sql(`INSERT INTO observatory.identity_history FORMAT JSONEachRow\n${JSON.stringify(renamed)}`);
 await until(async()=> (await resolve([rows[0].did]))[0].handle==='renamed.test');
 await sql(`INSERT INTO observatory.identity_history FORMAT JSONEachRow\n${JSON.stringify(rows[0])}`);
 await new Promise(r=>setTimeout(r,2200));
 assert.equal((await resolve([rows[0].did]))[0].handle,'renamed.test');
 const invalid={...renamed,handle:'',observed_at:'2026-10-18 23:00:01.000'};
 await sql(`INSERT INTO observatory.identity_history FORMAT JSONEachRow\n${JSON.stringify(invalid)}`);
 await until(async()=> (await resolve([rows[0].did]))[0].handle===null);
 await stop();await start();
 assert.equal((await resolve([rows[0].did]))[0].handle,null);
 assert.equal((await resolve([rows[1].did]))[0].handle,'fixture1.test');
 const stats=await (await fetch(`${origin}/stats`)).json();assert.equal(stats.queue.total,0,'known/invalid mappings must never enqueue lookups');
 for(const bad of [[],Array(101).fill(rows[0].did),['not-a-did']]) {
  const r=await fetch(`${origin}/resolve`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(bad)});assert.equal(r.status,400);
 }
 const jobs=await (await fetch(`${origin}/api/identity-queue?limit=25`)).json();assert.ok(Array.isArray(jobs.jobs));
 console.log('PASS: batch ordering, old positive handles, event updates, replay protection, invalidation, restart persistence, no known-account jobs, bounded input, queue API.');
} finally { await stop();await rm(root,{recursive:true,force:true}); }
