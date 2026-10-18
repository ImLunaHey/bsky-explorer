export async function api<T>(path:string):Promise<T> {
  const response = await fetch(path, { signal: AbortSignal.timeout(15_000) })
  if (!response.ok) throw new Error(response.status === 503 ? 'This summary is being refreshed. Please try again shortly.' : `Request failed (${response.status}). Please try again.`)
  return response.json() as Promise<T>
}
