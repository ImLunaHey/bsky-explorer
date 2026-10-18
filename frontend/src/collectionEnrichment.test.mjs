import test from 'node:test'
import assert from 'node:assert/strict'
import { collectionInfo, enrichRecord, safeWebUrl } from './collectionEnrichment.ts'

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
