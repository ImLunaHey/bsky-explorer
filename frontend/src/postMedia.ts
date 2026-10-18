import type { RecordMediaResponse } from './types'

// Per-post query keys, with coalesced metadata requests for nearby cards.
export function createMediaBatcher(fetchBatch: (uris: string[]) => Promise<{items: ({uri: string; found: boolean}&Pick<RecordMediaResponse,'media'|'counts'|'quote'>)[]}>) {
  let pending: {uri: string; resolve: (value: RecordMediaResponse & {found: boolean}) => void; reject: (reason: unknown) => void}[] = []
  let timer: ReturnType<typeof setTimeout> | undefined
  return (uri: string) => new Promise<RecordMediaResponse & {found: boolean}>((resolve,reject) => {
    pending.push({uri,resolve,reject})
    timer ??= setTimeout(() => {
      const entries = pending; pending = []; timer = undefined
      for (let i=0;i<entries.length;i+=25) {
        const batch=entries.slice(i,i+25)
        void fetchBatch([...new Set(batch.map(entry=>entry.uri))]).then(result=>{
          const byUri=new Map(result.items.map(item=>[item.uri,item]))
          for(const entry of batch) {
            const item=byUri.get(entry.uri)
            if(item)entry.resolve({media:item.media,counts:item.counts,quote:item.quote,found:item.found,source:'public_appview',ephemeral:true})
            else entry.reject(new Error('Media response omitted a post'))
          }
        }).catch(error=>batch.forEach(entry=>entry.reject(error)))
      }
    },20)
  })
}
export const fetchPostMedia=createMediaBatcher(async uris=>{
  const response=await fetch('/api/records/media/batch',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({uris}),signal:AbortSignal.timeout(15_000)})
  if(!response.ok)throw new Error('Attachments unavailable')
  return response.json()
})
