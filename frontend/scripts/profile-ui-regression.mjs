import assert from 'node:assert/strict'
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH,args:['--no-sandbox'],headless:true})
const origin=process.env.TEST_ORIGIN || 'http://127.0.0.1:13002'
const did='did:plc:profiletest'
const base=`${origin}/accounts/${encodeURIComponent(did)}`
const uri=i=>`at://${did}/app.bsky.feed.post/post${i}`
try {
 const page=await browser.newPage({viewport:{width:1280,height:1000}});const errors=[];const batches=[]
 page.on('pageerror',e=>{errors.push(e.message);console.error('Page error:',e.message)})
 await page.addInitScript(()=>{window.EventSource=class {addEventListener(){} close(){}}})
 await page.route('**/api/accounts/**',async route=>{
   const path=new URL(route.request().url()).pathname
   if(path.endsWith('/follows'))return route.fulfill({json:{windows:[],recent_events:[],unresolved_note:'Fixture'}})
   if(path.endsWith('/profile'))return route.fulfill({json:{handle:'profile.test',display_name:'Profile test',description:'Test profile',labels:[],followers_count:10,follows_count:5,posts_count:3}})
   if(path.endsWith('/posts/page')){const search=new URL(route.request().url()).searchParams;const filter=search.get('filter');const ids=search.has('before')?[4]:filter==='posts'?[1,2]:filter==='replies'?[3]:filter==='media'?[1,2]:[1,2,3];return route.fulfill({json:{posts:ids.map(i=>({at_uri:uri(i),actor_did:did,timestamp_ms:1700000000000,kind:i===3?'reply':'post',text:`Test post ${i}`,reply_parent:'',reply_root:'',quote_target:'',urls:[],domains:[],languages:['en']})),next:search.has('before')?null:{before:1700000000000,uri:uri(3)}}})}
   if(path.endsWith('/history'))return route.fulfill({json:{start_ms:1700000000000,end_ms:1700003600000,bucket_ms:3600000,note:'Fixture history',buckets:[{timestamp_ms:1700000000000,gained:1,lost:0,net:1,followed:1,unfollowed:0,unknown_unfollows:0,posts:1,replies:0,likes:1,reposts:0,other:0,likes_received:1,reposts_received:0,replies_received:0}]}})
   if(path.endsWith('/content')||path.endsWith('/events'))return route.fulfill({json:[]})
   return route.fulfill({json:{current_identity:{did,handle:'profile.test'},stored:null}})
 })
 await page.route('**/api/records/media/batch',route=>{
   const {uris}=route.request().postDataJSON();batches.push(uris)
   return route.fulfill({json:{items:uris.map(value=>({uri:value,found:true,media:value===uri(1)?[{kind:'image',url:origin+'/fixture.svg',alt:'A test album cover',width:400,height:200}]:value===uri(2)?[{kind:'video',url:origin+'/fixture.mp4',thumbnail_url:origin+'/fixture.svg',alt:'A test video'}]:[]})),source:'public_appview',ephemeral:true}})
 })
 await page.route('**/fixture.svg',route=>route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200"><rect width="400" height="200" fill="orange"/></svg>'}))
 await page.route('**/fixture.mp4',route=>route.fulfill({status:204}))
 await page.goto(base)
 await page.getByRole('button',{name:'7 days',exact:true}).click()
 assert.equal(new URL(page.url()).searchParams.get('time'),'7d')
 await page.getByRole('tab',{name:'Posts',exact:true}).click()
 assert.ok(new URL(page.url()).pathname.endsWith('/posts'))
 assert.equal(new URL(page.url()).searchParams.get('time'),'7d')
 await page.getByRole('img',{name:'A test album cover',exact:true}).waitFor()
 await page.locator('video[aria-label="A test video"]').scrollIntoViewIfNeeded()
 assert.equal(await page.locator('video').getAttribute('controls'),'')
 await page.getByRole('button',{name:'Posts',exact:true}).click()
 assert.equal(new URL(page.url()).searchParams.get('posts'),'posts')
 assert.equal(await page.getByText('Test post 3',{exact:true}).count(),0)
 await page.reload()
 await page.getByRole('heading',{name:'Recent posts',exact:true}).waitFor()
 assert.equal(await page.getByRole('button',{name:'Posts',exact:true}).getAttribute('aria-pressed'),'true')
 await page.getByRole('button',{name:'Load more retained posts',exact:true}).click()
 await page.getByText('Test post 4',{exact:true}).waitFor()
 await page.getByRole('button',{name:'Media',exact:true}).click()
 assert.equal(new URL(page.url()).searchParams.get('posts'),'media')
 await page.getByRole('tab',{name:'Audience',exact:true}).click()
 await page.waitForURL('**/audience?*')
 await page.getByRole('heading',{name:'Audience changes',exact:true}).waitFor()
 await page.getByLabel('Previous period comparison').first().waitFor()
 await page.getByRole('button',{name:'30 days',exact:true}).click()
 assert.equal(new URL(page.url()).searchParams.get('time'),'30d')
 await page.goBack()
 assert.equal(await page.getByRole('button',{name:'7 days',exact:true}).getAttribute('aria-pressed'),'true')
 await page.goBack()
 await page.getByRole('heading',{name:'Recent posts',exact:true}).waitFor()
 assert.equal(new URL(page.url()).searchParams.get('time'),'7d')
 await page.goForward()
 await page.getByRole('heading',{name:'Audience changes',exact:true}).waitFor()
 assert.ok(batches.length>0 && batches.every(batch=>batch.length<=25))
 assert.deepEqual(errors,[])
 console.log(JSON.stringify({tabsAndTimeInUrl:true,backForwardAndReload:true,repliesPersisted:true,imagesAndVideoControls:true,mediaBatchSizes:batches.map(b=>b.length),pageErrors:errors}))
} finally {await browser.close()}
