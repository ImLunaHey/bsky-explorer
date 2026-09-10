import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { FirehoseEvent, FirehoseFeed, Rates } from './types'

export type LiveConnection = {
  state: 'connecting' | 'connected' | 'disconnected'
  lastEventAt?: number
}

const MAX_FIREHOSE_EVENTS = 300

export function LiveUpdates() {
  const queryClient = useQueryClient()

  useEffect(() => {
    let closed = false
    queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connecting' })
    const source = new EventSource('/api/live')

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
        queryClient.setQueryData<FirehoseFeed>(['firehose-feed'], current => {
          if (!current) return {
            events: [incoming],
            next_after_time_us: incoming.time_us,
            retained_event_count: 1,
            has_more: false,
          }
          const alreadyPresent = current.events.some(item => item.event_id === incoming.event_id)
          const events = alreadyPresent ? current.events : [...current.events, incoming].slice(-MAX_FIREHOSE_EVENTS)
          return {
            ...current,
            events,
            next_after_time_us: Math.max(current.next_after_time_us ?? 0, incoming.time_us),
            retained_event_count: alreadyPresent ? current.retained_event_count : Math.max(current.retained_event_count + 1, events.length),
          }
        })
        queryClient.setQueryData<LiveConnection>(['live-connection'], { state: 'connected', lastEventAt: Date.now() })
      } catch { /* Ignore an invalid frame; EventSource remains connected. */ }
    })

    return () => { closed = true; source.close() }
  }, [queryClient])

  return null
}
