import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import ts from 'typescript'
import { QueryClient } from '@tanstack/react-query'
const require = createRequire(import.meta.url)
const code = ts.transpileModule(readFileSync(new URL('./identityQueries.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
const module = { exports: {} }
new Function('module', 'exports', 'require', code)(module, module.exports, name => name === './api' ? { api: () => { throw Error('Unexpected HTTP request') } } : require(name))
const { createIdentityBatcher, identityQueryOptions, needsIdentityLookup } = module.exports
const identity = did => ({ did, handle: 'person.test', source: 'plc', observed_at: '' })

test('per-DID misses coalesce into batches of at most 100 and deduplicate', async () => {
  const calls = []
  const fetch = createIdentityBatcher(async dids => { calls.push(dids); return dids.map(identity) })
  const first = fetch('did:plc:0')
  assert.equal(fetch('did:plc:0'), first)
  await Promise.all(Array.from({ length: 201 }, (_, i) => fetch(`did:plc:${i}`)))
  assert.deepEqual(calls.map(x => x.length), [100, 100, 1])
})
test('failed and omitted results release in-flight slots for retry', async () => {
  let attempt = 0
  const fetch = createIdentityBatcher(async dids => {
    if (++attempt === 1) throw Error('offline')
    if (attempt === 2) return []
    return dids.map(identity)
  })
  await assert.rejects(fetch('did:plc:a'), /offline/)
  await assert.rejects(fetch('did:plc:a'), /omitted/)
  assert.equal((await fetch('did:plc:a')).handle, 'person.test')
})
test('resolved DIDs are reused across observers and routes without another fetch', async () => {
  const client = new QueryClient()
  let calls = 0
  const options = { ...identityQueryOptions('did:plc:a'), queryFn: async () => { calls++; return identity('did:plc:a') } }
  await client.fetchQuery(options)
  await client.fetchQuery(options)
  assert.equal(calls, 1)
  assert.equal(options.gcTime, Infinity)
  assert.equal(options.refetchOnWindowFocus, false)
  assert.equal(options.refetchOnReconnect, false)
  client.clear()
})
test('only unknown identities poll; resolved and explicitly invalid identities stop', () => {
  const options = identityQueryOptions('did:plc:a')
  const interval = (data, extra = {}) => options.refetchInterval({ state: { data, dataUpdateCount: 1, ...extra } })
  assert.equal(interval(undefined), 2000)
  assert.equal(interval({ source: 'unresolved' }), 2000)
  assert.equal(interval(undefined, { error: Error('offline') }), 10000)
  assert.equal(interval(identity('did:plc:a')), false)
  assert.equal(interval({ source: 'jetstream_invalid', handle: null }), false)
  assert.equal(needsIdentityLookup({ source: 'provided', handle: 'known.test' }), false)
  assert.equal(options.refetchIntervalInBackground, false)
})
