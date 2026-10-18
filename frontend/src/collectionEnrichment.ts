// Deliberately small, explicit adapters. Unknown schemas retain a safe text
// preview; we never render arbitrary HTML or turn arbitrary URI schemes into links.
export const categories = ['All', 'Music', 'Publishing', 'Bookmarks', 'Video & live', 'Events', 'Culture', 'Science & data', 'Social', 'Other'] as const
export type Category = typeof categories[number]
export function collectionInfo(collection: string): { category: Category; label: string; description: string } {
  const rules: [string[], Category, string, string][] = [
    [['fm.teal.', 'app.rocksky.'], 'Music', 'Music & listening', 'Plays, artists, albums, playlists and listening status. Only play records contribute to charts.'],
    [['site.standard.', 'com.whtwnd.', 'tech.transparencia.news.'], 'Publishing', 'Articles & writing', 'Long-form writing, publications and articles shared through repositories.'],
    [['is.currents.', 'community.lexicon.bookmarks.'], 'Bookmarks', 'Saved discoveries', 'Links, collections and saved content. A save is not necessarily an endorsement.'],
    [['place.stream.', 'video.'], 'Video & live', 'Streams & video', 'Stream announcements and video records. A recent announcement does not prove a stream is live.'],
    [['community.lexicon.calendar.'], 'Events', 'Events & gatherings', 'Public event details, schedules and attendance records.'],
    [['social.popfeed.', 'com.puzzmo.', 'at.podping.'], 'Culture', 'Culture & play', 'Films, reading, games and podcast updates.'],
    [['dev.sensorthings.', 'net.atmowx.', 'at.adsb.', 'org.hypercerts.'], 'Science & data', 'Observations & open data', 'Structured observations, measurements and public research records.'],
    [['app.bsky.', 'town.delve.', 'social.coves.', 'so.sprk.'], 'Social', 'Social & communities', 'Posts, discussions, reactions and social connections.'],
  ]
  const found = rules.find(([prefixes]) => prefixes.some(prefix => collection.startsWith(prefix)))
  return found ? { category: found[1], label: found[2], description: found[3] } : { category: 'Other', label: 'Public repository records', description: 'An observed ATProto collection. Open a record to inspect its retained fields.' }
}
type Json = Record<string, unknown>
const object = (value: unknown): Json => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Json : {}
const text = (value: unknown, max = 500) => typeof value === 'string' ? value.trim().slice(0, max) : ''
export function safeWebUrl(value: unknown): string | undefined {
  if (typeof value !== 'string') return
  try { const url = new URL(value); if (['http:', 'https:'].includes(url.protocol) && !url.username && !url.password) return url.href } catch { /* not a web URL */ }
}
export type RecordPreview = { title: string; subtitle: string; body: string; tags: string[]; url?: string; date?: string }
export function enrichRecord(collection: string, raw: string, operation = 'create'): RecordPreview {
  const empty = { title: operation === 'delete' ? 'Record deleted' : 'Structured record', subtitle: collectionInfo(collection).label, body: '', tags: [] as string[] }
  if (operation === 'delete') return empty
  let data: Json
  try { data = object(JSON.parse(raw)) } catch { return empty }
  const tags = (Array.isArray(data.tags) ? data.tags : Array.isArray(data.genres) ? data.genres : []).map(t => text(t, 60)).filter(Boolean).slice(0, 6)
  const preview: RecordPreview = { ...empty, title: text(data.title) || text(data.name) || text(data.displayName) || empty.title, body: text(data.description) || text(data.textContent) || text(data.text) || text(data.content), tags, url: safeWebUrl(data.url) || safeWebUrl(data.originUrl) }
  if (collection.startsWith('fm.teal.') || collection.startsWith('app.rocksky.')) {
    const item = collection.endsWith('.status') ? object(data.item || data.track) : data
    const artists = Array.isArray(item.artists) ? item.artists.map(a => text(object(a).artistName)).filter(Boolean).join(' · ') : text(item.artist)
    preview.title = text(item.trackName) || text(item.title) || text(item.name) || preview.title
    preview.subtitle = [artists, text(item.releaseName) || text(item.album)].filter(Boolean).join(' / ') || empty.subtitle
    preview.date = text(item.playedTime) || text(item.createdAt)
    preview.url = safeWebUrl(item.originUri) || safeWebUrl(item.originUrl) || safeWebUrl(item.spotifyLink)
    preview.tags = [collection.endsWith('.status') ? 'Listening status · not a play' : collection.endsWith('.play') || collection.endsWith('.scrobble') ? 'Recorded play' : 'Music catalog', ...tags]
  } else if (collection === 'site.standard.document') {
    preview.subtitle = text(data.site) || 'Published document'
    // site may be an AT URI, not a web origin; don't invent a public URL.
    const site = safeWebUrl(data.site)
    if (site && typeof data.path === 'string' && data.path.startsWith('/') && !data.path.startsWith('//')) preview.url = safeWebUrl(new URL(data.path, site).href)
    preview.date = text(data.publishedAt)
  } else if (collection === 'community.lexicon.calendar.event') {
    const locations = Array.isArray(data.locations) ? data.locations : []
    preview.subtitle = locations.map(l => text(object(l).name) || text(object(l).locality)).filter((s, i, a) => s && a.indexOf(s) === i).join(' · ') || 'Public event'
    preview.date = text(data.startsAt)
    const uris = Array.isArray(data.uris) ? data.uris : []
    preview.url = safeWebUrl(object(uris[0]).uri)
  } else if (collection === 'is.currents.feed.save') {
    preview.url = safeWebUrl(data.originUrl)
    preview.title = text(data.title) || (preview.url ? new URL(preview.url).hostname : 'Saved content')
    preview.subtitle = 'Saved discovery'
  } else if (collection === 'place.stream.livestream') {
    preview.subtitle = 'Stream announcement · availability unverified'
    preview.date = text(data.lastSeenAt) || text(data.createdAt)
  } else if (collection === 'social.popfeed.feed.listItem') {
    preview.subtitle = [text(data.mainCredit), text(data.creativeWorkType), text(data.listType).replaceAll('_', ' ')].filter(Boolean).join(' · ')
  } else if (collection === 'at.podping.records.podping') {
    const uris = Array.isArray(data.uris) ? data.uris : []
    preview.url = safeWebUrl(uris[0]); preview.title = preview.url ? new URL(preview.url).hostname : 'Podcast feed update'
    preview.subtitle = `${uris.length} feed URLs · ${text(data.reason) || 'update'}`
  } else if (collection === 'dev.sensorthings.observationBatch') {
    preview.title = 'Sensor observation batch'
    preview.subtitle = `${Array.isArray(data.observations) ? data.observations.length : 0} measurements`
    preview.body = text(data.datastream)
    preview.date = text(data.windowEnd)
    preview.tags = ['Units and scaling belong to the datastream']
  } else if (collection === 'net.atmowx.observation') {
    preview.title = 'Weather station observation'
    preview.subtitle = 'Structured weather measurements'
    preview.body = ['temperature', 'relativeHumidity', 'wind', 'precipitation', 'pressureSeaLevel', 'uvIndex'].filter(key => key in data).join(' · ')
    preview.date = text(data.observedAt)
  } else if (collection === 'at.adsb.flight.record') {
    preview.title = text(data.callsign) || 'Aircraft observation'
    preview.subtitle = 'Observed flight record'
    preview.body = typeof data.messageCount === 'number' ? `${data.messageCount.toLocaleString()} messages · ${typeof data.positionCount === 'number' ? data.positionCount.toLocaleString() : 'unknown'} positions` : ''
    preview.date = text(data.lastSeen)
  } else if (collection === 'com.puzzmo.streak') {
    preview.title = text(data.gameDisplayName) || 'Game streak'
    preview.subtitle = typeof data.current === 'number' ? `Current streak: ${data.current}` : 'Recorded game progress'
    preview.body = typeof data.max === 'number' && typeof data.total === 'number' ? `Best streak: ${data.max} · Total: ${data.total}` : ''
    preview.date = text(data.lastUpdated)
  }
  if (preview.title === 'Structured record' && !preview.body) preview.body = `Fields: ${Object.keys(data).filter(key => key !== '$type').slice(0, 12).join(', ') || 'No retained fields'}`
  return preview
}
