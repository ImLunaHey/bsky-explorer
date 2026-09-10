const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/
const YOUTUBE_HOSTS = new Set(['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'])

export type YouTubeVideo = { id:string; originalUrl:string }

/** Parse only known YouTube video URL shapes; never returns arbitrary embed URLs. */
export function youtubeVideoId(value:string):string|undefined {
  let url:URL
  try { url=new URL(value) } catch { return undefined }
  if(url.protocol!=='https:'&&url.protocol!=='http:')return undefined
  const host=url.hostname.toLowerCase()
  let candidate:string|undefined
  if(host==='youtu.be') {
    const segments=url.pathname.split('/').filter(Boolean)
    if(segments.length===1)candidate=segments[0]
  } else if(YOUTUBE_HOSTS.has(host)) {
    const segments=url.pathname.split('/').filter(Boolean)
    if(url.pathname==='/watch')candidate=url.searchParams.get('v')||undefined
    else if(segments.length===2&&['shorts','embed'].includes(segments[0]))candidate=segments[1]
  }
  return candidate&&VIDEO_ID.test(candidate)?candidate:undefined
}

export function youtubeVideos(urls:string[],limit=3):YouTubeVideo[] {
  const seen=new Set<string>(),videos:YouTubeVideo[]=[]
  for(const originalUrl of urls) {
    const id=youtubeVideoId(originalUrl)
    if(!id||seen.has(id))continue
    seen.add(id);videos.push({id,originalUrl})
    if(videos.length>=limit)break
  }
  return videos
}
