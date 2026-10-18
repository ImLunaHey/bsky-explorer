import { Link } from '@tanstack/react-router'
export function AdvancedPage({admin=false}:{admin?:boolean}) {
  const links = admin ? [
    {to:'/system',name:'System health',description:'Inspect server resources, storage and service health.'},
    {to:'/system/network',name:'Ingestion diagnostics',description:'Check incoming traffic, lag and ingestion rates.'},
    {to:'/backfills',name:'History collection',description:'Manage collection jobs and inspect their progress.'},
    {to:'/identities',name:'Identity resolution',description:'Monitor the queue that turns account IDs into readable profiles.'},
  ] as const : [
    {to:'/network',name:'Network activity',description:'Explore measured activity and trends in the retained dataset.'},
    {to:'/collections',name:'Collections & records',description:'Browse schemas and inspect the underlying public records.'},
    {to:'/firehose',name:'Live record stream',description:'Watch incoming repository events. Useful for developers, not a curated feed.'},
    {to:'/coverage',name:'Coverage & limitations',description:'Understand what is retained, how fresh it is and what is missing.'},
    {to:'/communities',name:'Experimental connection graph',description:'Investigate algorithmic groups of Bluesky interactions—not named or declared communities.'},
    {to:'/detections',name:'Coordination research',description:'Inspect patterns and evidence. Scores do not establish intent or wrongdoing.'},
  ] as const
  return <div className="space-y-7"><Link to="/" className="text-sm text-dim">← Back to discovering</Link><header><p className="eyebrow">{admin?'For operators':'For curious minds & developers'}</p><h1 className="mt-3 text-3xl font-semibold">{admin?'Administration':'Look under the hood.'}</h1><p className="mt-3 max-w-2xl text-sm leading-7 text-dim">{admin?'Operational tools for this installation. Moving them here does not add an authentication boundary.':'The public experience starts with people and content. These tools expose the data and methods behind it.'}</p></header><div className="grid gap-4 sm:grid-cols-2">{links.map(item=><Link key={item.to} to={item.to} search={item.to==='/collections'?{collection:''}:undefined} className="discovery-card p-6"><h2 className="text-lg font-medium">{item.name} →</h2><p className="mt-3 text-sm leading-6 text-dim">{item.description}</p></Link>)}</div>{!admin && <Link to="/admin" className="inline-block text-sm text-dim">Installation administration →</Link>}</div>
}
