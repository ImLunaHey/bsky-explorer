import { useEffect, useRef, useState } from 'react'

export function HlsVideo({src,poster,label,className}:{src:string;poster?:string;label:string;className?:string}) {
  const video=useRef<HTMLVideoElement>(null),host=useRef<HTMLDivElement>(null)
  const [visible,setVisible]=useState(false),[failed,setFailed]=useState(false)
  useEffect(()=>{const node=host.current;if(!node)return;const observer=new IntersectionObserver(entries=>{if(entries.some(entry=>entry.isIntersecting)){setVisible(true);observer.disconnect()}},{rootMargin:'160px'});observer.observe(node);return()=>observer.disconnect()},[])
  useEffect(()=>{
    const element=video.current
    if(!visible||!element)return
    let cancelled=false,destroy:(()=>void)|undefined
    setFailed(false)
    const playlist=/\.m3u8(?:$|[?#])/i.test(src)
    if(!playlist||element.canPlayType('application/vnd.apple.mpegurl'))element.src=src
    else void import('hls.js').then(({default:Hls})=>{
      if(cancelled)return
      if(!Hls.isSupported()){setFailed(true);return}
      const hls=new Hls({enableWorker:true});destroy=()=>hls.destroy();hls.loadSource(src);hls.attachMedia(element);hls.on(Hls.Events.ERROR,(_event,data)=>{if(data.fatal)setFailed(true)})
    }).catch(()=>setFailed(true))
    return()=>{cancelled=true;destroy?.();element.removeAttribute('src');element.load()}
  },[src,visible])
  return <div ref={host} className={`relative grid place-items-center bg-black ${className||''}`}><video ref={video} poster={poster} aria-label={label} controls preload="metadata" playsInline onError={()=>setFailed(true)} className="h-full w-full object-contain"/>{failed&&<p className="absolute inset-x-2 bottom-2 rounded bg-black/80 p-2 text-center text-xs text-red-200">This video playlist could not be played.</p>}</div>
}
