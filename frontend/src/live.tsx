import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { FirehoseEvent, FirehoseFeed, Rates } from './types'

export type LiveConnection = {
  state: 'connecting' | 'connected' | 'disconnected'
  lastEventAt?: number
}

const MAX_FIREHOSE_EVENTS = 300

export function LiveUpdates() {
  const queryClient = useQueryClient()
  const pendingEvents = useRef<FirehoseEvent[]>([])

  useEffect(() => {
    let closed = false
    queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connecting' })
    const source = new EventSource('/api/live')
    const flush = window.setInterval(() => {
      const incoming = pendingEvents.current.splice(0)
      if (!incoming.length) return
      queryClient.setQueryData<FirehoseFeed>(['firehose-feed'], current => {
        const combined = [...(current?.events ?? []), ...incoming]
        const seen = new Set<string>()
        const events = combined.reverse().filter(item => !seen.has(item.event_id) && Boolean(seen.add(item.event_id))).reverse().slice(-MAX_FIREHOSE_EVENTS)
        const newest = incoming[incoming.length - 1]
        return {
          events,
          next_after_time_us: Math.max(current?.next_after_time_us ?? 0, newest.time_us),
          retained_event_count: Math.max((current?.retained_event_count ?? 0) + incoming.length, events.length),
          has_more: current?.has_more ?? false,
        }
      })
      queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connected', lastEventAt: Date.now() })
    }, 250)

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
      try {
        const incoming = JSON.parse(event.data) as FirehoseEvent
        pendingEvents.current.push(incoming)
        if (pendingEvents.current.length > 2_000) pendingEvents.current.splice(0, pendingEvents.current.length - 2_000)
      } catch { /* Ignore an invalid frame; EventSource remains connected. */ }
    })

    return () => { closed = true; window.clearInterval(flush); source.close() }
  }, [queryClient])

  return null
}
