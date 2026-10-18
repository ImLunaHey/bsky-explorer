import test from 'node:test'
import assert from 'node:assert/strict'
import { collectionInfo, enrichRecord, safeWebUrl, safeArtworkUrl } from './collectionEnrichment.ts'

test('artwork is extracted from music/status/film records and restricted to trusted HTTPS hosts', () => {
  const art = 'https://i.scdn.co/image/album'
  assert.equal(enrichRecord('app.rocksky.scrobble', JSON.stringify({albumArtUrl:art})).artworkUrl, art)
  assert.equal(enrichRecord('app.rocksky.actor.status', JSON.stringify({track:{albumCoverUrl:art}})).artworkUrl, art)
  assert.equal(enrichRecord('social.popfeed.feed.listItem', '{"posterUrl":"https://image.tmdb.org/t/p/w500/poster.jpg"}').artworkUrl, 'https://image.tmdb.org/t/p/w500/poster.jpg')
  assert.equal(enrichRecord('fm.teal.feed.play', '{}').artworkUrl, undefined)
  for (const url of ['http://i.scdn.co/a','https://localhost/a','https://127.0.0.1/a','https://i.scdn.co.evil.test/a','https://user:pass@i.scdn.co/a','https://i.scdn.co:8443/a','data:image/png,abc','javascript:alert(1)']) assert.equal(safeArtworkUrl(url), undefined)
})

test('known collections are categorized, unknown namespaces remain generic', () => {
  assert.equal(collectionInfo('fm.teal.feed.play').category, 'Music')
  assert.equal(collectionInfo('site.standard.document').category, 'Publishing')
  assert.equal(collectionInfo('fm.tealicious.fake').category, 'Other')
})
test('music cards normalize both schemas and distinguish status from plays', () => {
  const teal = enrichRecord('fm.teal.feed.play', JSON.stringify({ trackName: 'Song', artists: [{ artistName: 'Artist' }], releaseName: 'Album' }))
  const rocksky = enrichRecord('app.rocksky.scrobble', JSON.stringify({ title: 'Song', artist: 'Artist', album: 'Album' }))
  assert.equal(teal.title, rocksky.title)
  assert.equal(teal.subtitle, rocksky.subtitle)
  assert.match(enrichRecord('fm.teal.actor.status', '{"item":{"trackName":"Song"}}').tags[0], /not a play/)
})
test('unknown, malformed, deleted and null payloads degrade safely', () => {
  for (const raw of ['{', 'null', '[]', '"string"']) assert.equal(enrichRecord('org.example.record', raw).title, 'Structured record')
  assert.equal(enrichRecord('site.standard.document', '{"title":"Old"}', 'delete').title, 'Record deleted')
  assert.equal(enrichRecord('org.example.record', '{"title":"Readable title"}').title, 'Readable title')
})
test('external links allow only safe web URLs, never credentials or script schemes', () => {
  for (const url of ['javascript:alert(1)', 'data:text/html,hi', 'file:///etc/passwd', 'https://user:password@example.org', '//evil.test']) assert.equal(safeWebUrl(url), undefined)
  assert.equal(safeWebUrl('https://example.org/path'), 'https://example.org/path')
  assert.equal(enrichRecord('site.standard.document', '{"site":"at://did:plc:test/site.standard.publication/self","path":"/article"}').url, undefined)
  assert.equal(enrichRecord('site.standard.document', '{"site":"https://example.org","path":"//evil.test"}').url, undefined)
})
test('events, saved links and films expose useful fields', () => {
  assert.equal(enrichRecord('community.lexicon.calendar.event', '{"name":"Meetup","startsAt":"2026-10-20T12:00:00Z","locations":[{"name":"Town hall"}]}').subtitle, 'Town hall')
  assert.equal(enrichRecord('is.currents.feed.save', '{"originUrl":"https://example.org/article"}').title, 'example.org')
  assert.match(enrichRecord('social.popfeed.feed.listItem', '{"title":"Film","mainCredit":"Director","creativeWorkType":"movie"}').subtitle, /Director/)
})
test('observations and games are summarized without inventing measurement units', () => {
  const sensor = enrichRecord('dev.sensorthings.observationBatch', '{"observations":[{"result":{"value":2110}}]}')
  assert.equal(sensor.subtitle, '1 measurements')
  assert.equal(sensor.tags[0], 'Units and scaling belong to the datastream')
  assert.equal(enrichRecord('net.atmowx.observation', '{"temperature":{"value":51,"scale":-1}}').body, 'temperature')
  assert.equal(enrichRecord('com.puzzmo.streak', '{"gameDisplayName":"Crossword","current":4,"max":12,"total":20}').subtitle, 'Current streak: 4')
})
