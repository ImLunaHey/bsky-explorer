import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { HlsVideo } from './HlsVideo'
import { fetchPostMedia } from './postMedia'
import { Link } from '@tanstack/react-router'
import { IdentityLink } from './identities'

function PostImage({url,alt,width,height}:{url:string;alt:string;width?:number;height?:number}) {
  const [failed,setFailed]=useState(false)
  return failed ? <p className="border border-line p-4 text-xs text-dim">Image unavailable{alt ? `: ${alt}` : '.'}</p>
    : <a href={url} target="_blank" rel="noreferrer" aria-label={alt ? `Open image: ${alt}` : 'Open full-size image'}><img src={url} alt={alt} width={width} height={height} loading="lazy" referrerPolicy="no-referrer" onError={()=>setFailed(true)} className="max-h-[65vh] w-full bg-black object-contain" /></a>
}

export function PostMedia({uri,quoteTarget}:{uri:string;quoteTarget?:string}) {
  const host=useRef<HTMLDivElement>(null)
  const [visible,setVisible]=useState(false)
  useEffect(()=>{
    const node=host.current;if(!node)return
    const observer=new IntersectionObserver(entries=>{if(entries.some(entry=>entry.isIntersecting)){setVisible(true);observer.disconnect()}},{rootMargin:'300px'})
    observer.observe(node);return()=>observer.disconnect()
  },[])
  const query=useQuery({queryKey:['record-media',uri],queryFn:()=>fetchPostMedia(uri),enabled:visible,staleTime:300_000,gcTime:900_000,retry:1})
  return <div ref={host}>
    {query.isPending && <div role="status" aria-label="Loading post attachments" className="mt-3 h-5 w-36 animate-pulse bg-white/5"/>}
    {query.isError && <p className="mt-3 text-xs text-dim">Attachments couldn’t load. <button onClick={()=>void query.refetch()} className="text-cyan">Retry attachments</button></p>}
    {query.data?.found === false && <p className="mt-3 text-xs text-dim">The current public post is unavailable; its attachments may have been removed.</p>}
    {!!query.data?.media.length && <div className={`mt-4 grid gap-2 ${query.data.media.length>1?'sm:grid-cols-2':''}`}>
      {query.data.media.map((asset,index)=>asset.kind==='image'
        ? <PostImage key={asset.url} url={asset.url} alt={asset.alt} width={asset.width} height={asset.height}/>
        : <HlsVideo key={asset.url} src={asset.url} poster={asset.thumbnail_url} label={asset.alt||`Post video ${index+1}`} className="aspect-video max-h-[65vh] w-full"/>)}
    </div>}
    {(query.data?.quote||quoteTarget)&&<blockquote className="mt-4 border border-line p-4 text-sm">{query.data?.quote?.did&&<IdentityLink did={query.data.quote.did} handle={query.data.quote.handle||undefined} inline className="text-xs"/>}<Link to="/records/$uri" params={{uri:query.data?.quote?.uri||quoteTarget!}} className="mt-2 block whitespace-pre-wrap text-dim">{query.data?.quote?.text||'View quoted post →'}</Link></blockquote>}
    {query.data?.counts&&<p className="mt-4 flex flex-wrap gap-3 text-xs text-dim" aria-label="Current Bluesky engagement">{Object.entries(query.data.counts).map(([name,value])=><span key={name}>{value==null?'—':value.toLocaleString()} {name}</span>)}<span className="text-[10px]">Current public totals</span></p>}
  </div>
}
