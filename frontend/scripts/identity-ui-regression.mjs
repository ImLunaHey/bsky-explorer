const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
import assert from 'node:assert/strict'
const browser = await chromium.launch({executablePath:process.env.CHROMIUM_PATH,args:['--no-sandbox'],headless:true})
try {
 const page=await browser.newPage(); const errors=[]; page.on('pageerror',e=>errors.push(e.message))
 let resolved=false,calls=0; const did='did:plc:testidentitycache'
 await page.addInitScript(()=>{ window.EventSource=class { timers=[]; addEventListener(type, callback){ if(type==='rates')this.timers.push(setInterval(()=>callback({data:JSON.stringify({windows:[],events_per_sec:Math.random(),posts_per_sec:1,likes_per_sec:1,follows_per_sec:1})}),250)) } close(){this.timers.forEach(clearInterval)} } })
 await page.route('**/api/identities?*',async route=>{
   calls++; const dids=new URL(route.request().url()).searchParams.get('dids').split(',')
   await route.fulfill({json:dids.map(did=>({did,handle:resolved?'cached-person.test':null,source:resolved?'plc':'unresolved',observed_at:''}))})
 })
 await page.route('**/api/firehose?*',route=>route.fulfill({json:{events:[1,2].map(i=>({event_id:'test'+i,time_us:Date.now()*1000+i,actor_did:did,kind:'follow',collection:'app.bsky.graph.follow',timestamp:new Date().toISOString(),at_uri:'',urls:[],domains:[]})),retained_event_count:2,has_more:false}}))
 await page.goto(`${process.env.TEST_ORIGIN || 'http://127.0.0.1:13002'}/firehose`)
 await page.getByRole('button',{name:'Pause',exact:true}).click()
 await page.getByText('resolving…',{exact:true}).first().waitFor()
 assert.equal(calls,1,'two rows should share one lookup')
 const rows=await page.locator('tbody tr').count();
 const label=page.locator('tbody tr').first().locator('button').first().locator('span span').first()
 assert.equal(await label.evaluate(el=>getComputedStyle(el).fontSize),'10px')
 assert.equal(await label.evaluate(el=>getComputedStyle(el).whiteSpace),'nowrap')
 resolved=true
 await page.getByText('@cached-person.test',{exact:true}).first().waitFor({timeout:15000})
 assert.equal(await page.locator('tbody tr').count(),rows)
 assert.equal(await page.getByRole('button',{name:'Resume',exact:true}).count(),1)
 const after=calls
 await page.getByRole('link',{name:'Images',exact:true}).click()
 await page.getByRole('link',{name:'Events',exact:true}).click()
 await page.getByText('@cached-person.test',{exact:true}).first().waitFor()
 await page.waitForTimeout(4500)
 assert.equal(calls,after,'resolved identity must not refetch after route change or polling interval')
 assert.deepEqual(errors,[])
 console.log(JSON.stringify({pausedRowsStable:rows,labelsUpdatedWhilePaused:true,identityRequests:calls,noRefetchOnRemount:true,pageErrors:errors}))
} finally {await browser.close()}
