import type { RepositoryRecord } from './CollectionsPage'
import { enrichRecord, safeWebUrl } from './collectionEnrichment'

export const topics = [
  { id: 'music', label: 'Music', description: 'Find your next favourite song.' },
  { id: 'writing', label: 'Writing', description: 'Fresh perspectives from independent writers.' },
  { id: 'films', label: 'Films', description: 'See what people are watching and saving.' },
  { id: 'events', label: 'Events', description: 'Find something to look forward to.' },
  { id: 'bookmarks', label: 'Saved links', description: 'Discover what caught someone’s attention.' },
  { id: 'streams', label: 'Streams', description: 'Recent broadcasts worth checking out.' },
  { id: 'games', label: 'Games', description: 'Small victories and daily rituals.' },
] as const
export type Topic = typeof topics[number]['id']
export type Discovery = { generated_at: number; observed_since: number; sections: { category: string; candidate_records: number; records: RepositoryRecord[] }[] }
export type Ranking = { id: string; title: string; subtitle: string; plays: number; listeners: number; example_uri: string; sources: string[]; artwork_url?: string | null; listen_url?: string | null }
export type Music = { start: number; end: number; generated_at: number; observed_since: number; plays: number; listeners: number; tracks: Ranking[]; artists: Ranking[]; albums: Ranking[]; tracks_by_listeners: Ranking[]; artists_by_listeners: Ranking[]; albums_by_listeners: Ranking[]; source_records: Record<string, number>; duplicate_plays: number; unparseable_records: number; scanned_records: number; activity: [number, number][] }

// Curated descriptions and official destinations, not inferred from arbitrary NSIDs.
export const apps = [
  { id:'teal', name:'Teal', category:'Music', description:'Keep a listening history and discover music through other people’s taste.', url:'https://teal.fm', prefixes:['fm.teal.'], collection:'fm.teal.feed.play', topic:'music', mark:'T' },
  { id:'rocksky', name:'Rocksky', category:'Music', description:'Track the songs you play and explore artists, albums and listening histories.', url:'https://rocksky.app', prefixes:['app.rocksky.'], collection:'app.rocksky.scrobble', topic:'music', mark:'R' },
  { id:'whitewind', name:'WhiteWind', category:'Writing', description:'Publish long-form blog posts with your ATProto identity.', url:'https://whtwnd.com', prefixes:['com.whtwnd.'], collection:'com.whtwnd.blog.entry', topic:'writing', mark:'W' },
  { id:'currents', name:'Currents', category:'Saved links', description:'Collect inspiration from the web and organize it into visual collections.', url:'https://currents.is', prefixes:['is.currents.'], collection:'is.currents.feed.save', topic:'bookmarks', mark:'C' },
  { id:'streamplace', name:'Streamplace', category:'Streams', description:'Broadcast and watch live video on the open social web.', url:'https://stream.place', prefixes:['place.stream.'], collection:'place.stream.livestream', topic:'streams', mark:'S' },
  { id:'popfeed', name:'Popfeed', category:'Films & culture', description:'Log films, books, games and more. Share reviews and discover what others enjoy.', url:'https://popfeed.social', prefixes:['social.popfeed.'], collection:'social.popfeed.feed.listItem', topic:'films', mark:'P' },
  { id:'standard', name:'Standard.site', category:'Publishing format', description:'A shared publishing format used by multiple apps—not a single publication or app.', url:'https://standard.site', prefixes:['site.standard.'], collection:'site.standard.document', topic:'writing', mark:'Aa' },
] as const
export function appFor(collection: string) { return apps.find(app => app.prefixes.some(prefix => collection.startsWith(prefix))) }
export function recordData(record: RepositoryRecord): Record<string, unknown> {
  try { const value: unknown = JSON.parse(record.record_json); return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {} } catch { return {} }
}
export function topicFor(collection: string): Topic | undefined {
  if (collection.startsWith('fm.teal.') || collection.startsWith('app.rocksky.')) return 'music'
  if (collection === 'site.standard.document' || collection === 'com.whtwnd.blog.entry') return 'writing'
  return ({'social.popfeed.feed.listItem':'films','community.lexicon.calendar.event':'events','is.currents.feed.save':'bookmarks','place.stream.livestream':'streams','com.puzzmo.streak':'games'} as Record<string,Topic>)[collection]
}
export function presentRecord(record: RepositoryRecord) {
  const preview = enrichRecord(record.collection, record.record_json, record.operation)
  const data = recordData(record)
  const topic = topicFor(record.collection)
  if (topic === 'writing' && (preview.subtitle.startsWith('at://') || preview.subtitle === 'Public repository records')) preview.subtitle = appFor(record.collection)?.name || 'Independent publication'
  let action = topic === 'writing' ? 'Published an article' : topic === 'music' ? 'Shared a listen' : topic === 'events' ? 'Upcoming event' : topic === 'bookmarks' ? 'Saved a link' : topic === 'streams' ? 'Stream announcement' : topic === 'games' ? 'Game progress' : 'Shared an update'
  if (topic === 'films') action = ({watched_movies:'Watched',watchlist_movies:'Added to watchlist',want_to_watch_movies:'Wants to watch',favorite_movies:'Added to favourites'} as Record<string,string>)[String(data.listType)] || 'Added to a film list'
  if (record.operation === 'delete') action = 'No longer available'
  let url = preview.url
  if (topic === 'films' && data.identifiers && typeof data.identifiers === 'object') {
    const identifiers = data.identifiers as Record<string,unknown>
    if (typeof identifiers.imdbId === 'string' && /^tt\d+$/.test(identifiers.imdbId)) url = `https://www.imdb.com/title/${identifiers.imdbId}/`
    else if (/^\d+$/.test(String(identifiers.tmdbId || ''))) url = `https://www.themoviedb.org/movie/${identifiers.tmdbId}`
  }
  if (record.collection === 'com.whtwnd.blog.entry') url = `https://whtwnd.com/${encodeURIComponent(record.actor_did)}/${encodeURIComponent(record.at_uri.split('/').at(-1) || '')}`
  return { ...preview, url: safeWebUrl(url), topic, action, app: appFor(record.collection) }
}
