export const profileTabs = ['Overview', 'Audience', 'Activity', 'Posts', 'Writing', 'Listening', 'Apps'] as const
export type ProfileTab = typeof profileTabs[number]
export type ProfileTime = '24h' | '7d' | '30d'
export type PostFilter = 'all'|'posts'|'replies'|'media'
export type ProfileSearch = { time?: ProfileTime; replies?: false; posts?: PostFilter }
export function profileTab(section?: string): ProfileTab {
  return profileTabs.find(tab => tab.toLowerCase() === section) || 'Overview'
}
export function profileSearch(search: Record<string, unknown>): ProfileSearch {
  return {
    time: search.time === '24h' || search.time === '7d' || search.time === '30d' ? search.time : undefined,
    replies: search.replies === false || search.replies === 'false' ? false : undefined,
    ...(search.posts === 'all' || search.posts === 'posts' || search.posts === 'replies' || search.posts === 'media' ? {posts:search.posts} : {}),
  }
}
export const profileDays = (time?: ProfileTime) => time === '30d' ? 30 : time === '7d' ? 7 : 1
