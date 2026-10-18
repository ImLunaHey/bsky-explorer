import type { ReactNode } from 'react'

export function SkeletonBlock({className='h-3 w-full'}:{className?:string}) {
  return <div aria-hidden="true" className={`skeleton-block ${className}`}/>
}
function LoadingRegion({label,children}:{label:string;children:ReactNode}) {
  return <div role="status" aria-label={label} aria-busy="true"><span className="sr-only">{label}</span><div aria-hidden="true">{children}</div></div>
}
export function CardSkeletons({compact=false,count=3}:{compact?:boolean;count?:number}) {
  return <LoadingRegion label="Loading discoveries"><div className={`grid ${compact?'gap-3':'gap-4'} sm:grid-cols-2 lg:grid-cols-3`}>{Array.from({length:count},(_,i)=><div key={i} className={`discovery-card p-4 ${compact?'flex min-h-32 items-center gap-4':'min-h-64 space-y-4 p-5'}`}><SkeletonBlock className={compact?'size-16 shrink-0':'h-3 w-28'}/><div className="min-w-0 flex-1 space-y-3"><SkeletonBlock className="h-5 w-3/4"/><SkeletonBlock className="h-3 w-full"/><SkeletonBlock className="h-3 w-2/3"/>{!compact && <><SkeletonBlock className="mt-6 h-16 w-full"/><SkeletonBlock className="mt-5 h-3 w-24"/></>}</div></div>)}</div></LoadingRegion>
}
export function ListSkeleton({rows=5,artwork=false}:{rows?:number;artwork?:boolean}) {
  return <LoadingRegion label={artwork?'Loading music':'Loading results'}><div className="panel divide-y divide-line">{Array.from({length:rows},(_,i)=><div key={i} className="flex min-h-24 items-center gap-4 p-4"><SkeletonBlock className={artwork?'size-16 shrink-0':'size-10 shrink-0'}/><div className="min-w-0 flex-1 space-y-3"><SkeletonBlock className="h-4 w-2/3"/><SkeletonBlock className="h-3 w-1/2"/></div><SkeletonBlock className="h-3 w-12 shrink-0"/></div>)}</div></LoadingRegion>
}
export function AnalyticsSkeleton() {
  return <LoadingRegion label="Loading charts and statistics"><div className="space-y-5"><div className="grid grid-cols-2 gap-3 xl:grid-cols-4">{[0,1,2,3].map(i=><div key={i} className="panel p-4"><SkeletonBlock className="h-3 w-24"/><SkeletonBlock className="mt-3 h-9 w-20"/></div>)}</div><SkeletonBlock className="h-10 w-full"/><div className="grid gap-5 xl:grid-cols-2">{[0,1].map(i=><div key={i} className="panel p-5"><SkeletonBlock className="h-5 w-48"/><SkeletonBlock className="mt-3 h-3 w-3/4"/><SkeletonBlock className="mt-5 h-3 w-2/3"/><div className="skeleton-chart mt-5 aspect-[760/210] min-h-32 border-b border-l border-line"/><SkeletonBlock className="mt-5 h-3 w-3/4"/><SkeletonBlock className="mt-5 h-3 w-24"/></div>)}</div></div></LoadingRegion>
}
export function ProfileHeaderSkeleton() {
  return <LoadingRegion label="Loading profile"><div className="p-5"><div className="flex items-end justify-between"><SkeletonBlock className="size-16"/><SkeletonBlock className="h-8 w-32"/></div><div className="mt-4 grid gap-5 lg:grid-cols-2"><div className="space-y-3"><SkeletonBlock className="h-9 w-48"/><SkeletonBlock className="h-4 w-36"/><SkeletonBlock className="mt-4 h-24 w-full max-w-lg"/></div><div className="lg:border-l lg:border-line lg:pl-7"><div className="grid grid-cols-3 gap-3">{[0,1,2].map(i=><div key={i}><SkeletonBlock className="h-7 w-16"/><SkeletonBlock className="mt-2 h-3 w-14"/></div>)}</div><SkeletonBlock className="mt-5 h-3 w-3/4"/><SkeletonBlock className="mt-5 h-3 w-1/2"/></div></div></div></LoadingRegion>
}
export function DetailSkeleton({music=false}:{music?:boolean}) {
  return <LoadingRegion label="Loading details"><div className="space-y-6"><div className="panel flex flex-col gap-6 p-6 sm:flex-row">{music && <SkeletonBlock className="size-44 shrink-0 sm:size-48"/>}<div className="min-w-0 flex-1 space-y-4"><SkeletonBlock className="h-3 w-24"/><SkeletonBlock className="h-10 w-3/4"/><SkeletonBlock className="h-5 w-1/2"/><SkeletonBlock className="mt-6 h-10 w-40"/></div></div><div className="panel space-y-5 p-6">{[0,1,2,3].map(i=><SkeletonBlock key={i} className={`h-5 ${i===3?'w-2/3':'w-full'}`}/>)}</div></div></LoadingRegion>
}
export function PageSkeleton({view}:{view:string}) {
  if(view==='account')return <div className="space-y-6"><div className="panel"><ProfileHeaderSkeleton/><div className="flex gap-4 border-t border-line p-4" aria-hidden="true">{[0,1,2,3].map(i=><SkeletonBlock key={i} className="h-5 w-16"/>)}</div></div><AnalyticsSkeleton/></div>
  if(view==='music-detail'||view==='story'||view==='record')return <DetailSkeleton music={view==='music-detail'}/>
  return <div className="space-y-6"><div aria-hidden="true" className="space-y-4 py-4"><SkeletonBlock className="h-3 w-32"/><SkeletonBlock className="h-10 w-2/3 max-w-lg"/><SkeletonBlock className="h-4 w-3/4 max-w-xl"/></div>{view==='network'?<AnalyticsSkeleton/>:view==='music'||view==='search'?<ListSkeleton artwork={view==='music'}/>:<CardSkeletons count={6}/>}</div>
}
