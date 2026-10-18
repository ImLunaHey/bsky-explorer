import { Link } from '@tanstack/react-router'

export function CoverageNote({kind='retained',children}:{kind?:'retained'|'current'|'live';children?:React.ReactNode}) {
  return <aside className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-l-2 border-cyan/60 bg-white/[.025] px-4 py-3 text-xs leading-5 text-dim">
    <strong className="font-medium text-white">{kind==='current'?'Current public totals':kind==='live'?'Observed live activity':'Retained history'}</strong>
    <span>{children || (kind==='current'?'A current public snapshot, not a historical measurement.':kind==='live'?'Events received by this explorer—not a complete view of every ATProto service.':'Only records collected by this explorer. Missing records do not prove inactivity.')}</span>
    <Link to="/coverage" className="text-cyan underline underline-offset-2">Coverage & limitations</Link>
  </aside>
}
