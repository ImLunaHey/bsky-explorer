import type { FirehoseEvent } from './types'
export function eventAction(event: Pick<FirehoseEvent,'kind'|'collection'>) {
  return ({post:'Published a post',reply:'Replied to a post',quote:'Quoted a post',like:'Liked a post',repost:'Reposted a post',follow:'Followed',unfollow:'Unfollowed',block:'Blocked',unblock:'Unblocked',identity:'Updated their identity',account:'Updated their account'} as Record<string,string>)[event.kind] || `Updated ${event.collection || 'a repository record'}`
}
export function eventTarget(event:Pick<FirehoseEvent,'target_did'|'target'>) {
  return event.target_did || (event.target?.startsWith('at://')?event.target.slice(5).split('/')[0]:event.target?.startsWith('did:')?event.target:undefined)
}
