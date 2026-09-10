import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { FirehoseEvent, FirehoseFeed, Rates } from './types'

export type LiveConnection = {
  state: 'connecting' | 'connected' | 'disconnected'
  lastEventAt?: number
}

// Keep several pages of events client-side. This is deliberately bounded: at
// typical normalized-event sizes it is only a few megabytes, while allowing
// people to page back without another request or moving rows underneath them.
const MAX_FIREHOSE_EVENTS = 5_000
const MAX_PENDING_EVENTS = 5_000
const FLUSH_INTERVAL_MS = 500

export function LiveUpdates() {
  const queryClient = useQueryClient()
  const pendingEvents = useRef<FirehoseEvent[]>([])
  const knownEventIds = useRef(new Set<string>())
  const lastCachedEventId = useRef<string | undefined>(undefined)

  useEffect(() => {
    let closed = false
    queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connecting' })
    const source = new EventSource('/api/live')
    const flush = window.setInterval(() => {
      const incoming = pendingEvents.current.splice(0)
      if (!incoming.length) return
      queryClient.setQueryData<FirehoseFeed>(['firehose-feed'], current => {
        const existing = current?.events ?? []
        const currentLastId = existing.at(-1)?.event_id

        // A HTTP snapshot may have populated or replaced the cache since the
        // previous flush. Rebuild the index once in that case; steady-state
        // batches then only inspect new events instead of reversing and
        // filtering the entire cache every 250 ms.
        if (currentLastId !== lastCachedEventId.current) {
          knownEventIds.current = new Set(existing.map(event => event.event_id))
        }
        const uniqueIncoming = incoming.filter(event => {
          if (knownEventIds.current.has(event.event_id)) return false
          knownEventIds.current.add(event.event_id)
          return true
        })
        if (!uniqueIncoming.length) {
          lastCachedEventId.current = currentLastId
          return current
        }

        const combined = [...existing, ...uniqueIncoming]
        const events = combined.length > MAX_FIREHOSE_EVENTS
          ? combined.slice(combined.length - MAX_FIREHOSE_EVENTS)
          : combined
        if (combined.length !== events.length) {
          for (const event of combined.slice(0, combined.length - events.length)) {
            knownEventIds.current.delete(event.event_id)
          }
        }
        const newest = uniqueIncoming.at(-1)!
        lastCachedEventId.current = newest.event_id
        return {
          events,
          next_after_time_us: Math.max(current?.next_after_time_us ?? 0, newest.time_us),
          retained_event_count: Math.max((current?.retained_event_count ?? 0) + uniqueIncoming.length, events.length),
          has_more: current?.has_more ?? false,
        }
      })
      queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connected', lastEventAt: Date.now() })
    }, FLUSH_INTERVAL_MS)

    source.onopen = () => {
      if (!closed) queryClient.setQueryData<LiveConnection>(['live-connection'], current => ({ ...current, state: 'connected' }))
    }
    source.onerror = () => {
      if (!closed) queryClient.setQueryData<LiveConnection>(['live-connection'], current => ({ ...current, state: 'disconnected' }))
    }
    source.addEventListener('rates', event => {
      try {
        const rates = JSON.parse(event.data) as Rates
        queryClient.setQueryData(['rates'], rates)
        queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connected', lastEventAt: Date.now() })
      } catch { /* Ignore an invalid frame; EventSource remains connected. */ }
    })
    source.addEventListener('firehose', event => {
      // The global connection also carries rates. Avoid JSON parsing, cache
      // churn, and main-thread work for the high-volume feed on every other
      // route; the Firehose page catches up from its initial HTTP snapshot.
      if (window.location.pathname !== '/firehose') return
      try {
        const incoming = JSON.parse(event.data) as FirehoseEvent
        pendingEvents.current.push(incoming)
        if (pendingEvents.current.length > MAX_PENDING_EVENTS) {
          pendingEvents.current.splice(0, pendingEvents.current.length - MAX_PENDING_EVENTS)
        }
      } catch { /* Ignore an invalid frame; EventSource remains connected. */ }
    })

    return () => { closed = true; window.clearInterval(flush); source.close() }
  }, [queryClient])

  return null
}
