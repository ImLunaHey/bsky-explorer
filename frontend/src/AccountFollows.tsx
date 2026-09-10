import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowDownUp, ChevronLeft, ChevronRight } from "lucide-react";
import { api } from "./api";
import { shortDid, useIdentityMap } from "./identities";

export type FollowWindow = {
  seconds: number;
  outbound_follows: number;
  inbound_follows: number;
  outbound_unfollows_resolved: number;
  inbound_unfollows_resolved: number;
  outbound_unfollows_unresolved: number;
  outbound_net: number;
  inbound_net: number;
};

export type FollowEvent = {
  event_id: string;
  timestamp_ms: number;
  actor_did: string;
  target_did: string;
  kind: "follow" | "unfollow";
  direction: "inbound" | "outbound";
  target_resolved: boolean;
};

export type FollowAnalytics = {
  windows: FollowWindow[];
  recent_events: FollowEvent[];
  unresolved_note: string;
};
const number = new Intl.NumberFormat();
const windowName = (seconds: number) =>
  seconds < 3600
    ? `${seconds / 60}m`
    : seconds < 86400
      ? `${seconds / 3600}h`
      : `${seconds / 86400}d`;

function FollowEventTable({
  title,
  events,
  handles,
  onAccount,
}: {
  title: string;
  events: FollowEvent[];
  handles: Map<string, string | undefined>;
  onAccount: (did: string) => void;
}) {
  const [page, setPage] = useState(0);
  const [descending, setDescending] = useState(true);
  const pageSize = 10;
  const sorted = [...events].sort((a, b) =>
    descending ? b.timestamp_ms - a.timestamp_ms : a.timestamp_ms - b.timestamp_ms,
  );
  const pages = Math.max(1, Math.ceil(sorted.length / pageSize));
  const safePage = Math.min(page, pages - 1);
  const visible = sorted.slice(safePage * pageSize, (safePage + 1) * pageSize);
  return (
    <section className="min-w-0">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="eyebrow">{title}</h3>
        <span className="text-xs text-dim">{events.length} retained</span>
      </div>
      <div className="overflow-x-auto rounded-xl border border-line">
        <table className="w-full min-w-105 text-left text-xs">
          <thead className="border-b border-line text-dim">
            <tr><th className="px-3 py-3">Account</th><th className="px-3">Change</th><th className="px-3"><button className="flex items-center gap-1" onClick={()=>setDescending(value=>!value)}>Observed <ArrowDownUp className="size-3"/></button></th></tr>
          </thead>
          <tbody>{visible.map(event=>{const other=event.direction==='outbound'?event.target_did:event.actor_did;const handle=handles.get(other);return <tr key={event.event_id} className="border-b border-line/60 last:border-0"><td className="px-3 py-3">{other?<button onClick={()=>onAccount(other)} title={other} className="block max-w-52 text-left text-orange-300 hover:text-orange-200"><b className="block truncate">{handle?`@${handle}`:shortDid(other)}</b>{handle&&<span className="block truncate font-mono text-[10px] text-dim">{shortDid(other)}</span>}</button>:<span className="italic text-dim">Target unavailable</span>}</td><td className={event.kind==='follow'?'px-3 text-mint':'px-3 text-amber-300'}>{event.kind==='follow'?(event.direction==='inbound'?'Followed this account':'Followed'):(event.direction==='inbound'?'Stopped following':'Unfollowed')}</td><td className="whitespace-nowrap px-3 text-dim">{new Date(event.timestamp_ms).toLocaleString()}</td></tr>})}</tbody>
        </table>
        {!visible.length&&<div className="grid min-h-24 place-items-center text-sm text-dim">No {title.toLowerCase()} retained.</div>}
      </div>
      {sorted.length>pageSize&&<div className="mt-3 flex items-center justify-between text-xs text-dim"><span>Page {safePage+1} of {pages}</span><div className="flex gap-2"><button disabled={!safePage} onClick={()=>setPage(safePage-1)} className="rounded border border-line p-1.5 disabled:opacity-30" aria-label={`Previous ${title} page`}><ChevronLeft className="size-4"/></button><button disabled={safePage+1>=pages} onClick={()=>setPage(safePage+1)} className="rounded border border-line p-1.5 disabled:opacity-30" aria-label={`Next ${title} page`}><ChevronRight className="size-4"/></button></div></div>}
    </section>
  );
}

export function AccountFollows({
  did,
  onAccount,
}: {
  did: string;
  onAccount: (did: string) => void;
}) {
  const analytics = useQuery({
    queryKey: ["account-follows", did],
    queryFn: () =>
      api<FollowAnalytics>(`/api/accounts/${encodeURIComponent(did)}/follows`),
    staleTime: 15_000,
  });
  const handles=useIdentityMap((analytics.data?.recent_events||[]).map(event=>event.direction==='outbound'?event.target_did:event.actor_did).filter(Boolean));
  if (analytics.isLoading)
    return (
      <div className="mt-8 flex min-h-28 items-center justify-center text-sm text-dim">
        Loading follow activity…
      </div>
    );
  if (analytics.isError || !analytics.data)
    return (
      <div className="mt-8 rounded-xl border border-red-400/20 p-4 text-sm text-red-300">
        Follow activity is temporarily unavailable.
      </div>
    );
  const events = analytics.data.recent_events;
  return (
    <>
      <h3 className="eyebrow mb-3 mt-8">Follow activity</h3>
      <div className="scrollbar overflow-x-auto rounded-xl border border-line">
        <table className="w-full min-w-175 text-left text-xs">
          <thead className="border-b border-line text-dim">
            <tr>
              <th className="px-3 py-3">Window</th>
              <th className="px-3">Following</th>
              <th className="px-3">Followers</th>
              <th className="px-3">Unfollowed</th>
              <th className="px-3">Lost followers</th>
              <th className="px-3">Unknown target</th>
              <th className="px-3">Outbound net</th>
              <th className="px-3">Inbound net</th>
            </tr>
          </thead>
          <tbody>
            {analytics.data.windows.map((row) => (
              <tr
                key={row.seconds}
                className="border-b border-line/60 last:border-0"
              >
                <th className="px-3 py-3 text-cyan">
                  {windowName(row.seconds)}
                </th>
                <td className="px-3">{number.format(row.outbound_follows)}</td>
                <td className="px-3">{number.format(row.inbound_follows)}</td>
                <td className="px-3">
                  {number.format(row.outbound_unfollows_resolved)}
                </td>
                <td className="px-3">
                  {number.format(row.inbound_unfollows_resolved)}
                </td>
                <td className="px-3 text-amber-300">
                  {number.format(row.outbound_unfollows_unresolved)}
                </td>
                <td
                  className={`px-3 font-semibold ${row.outbound_net < 0 ? "text-red-300" : "text-mint"}`}
                >
                  {row.outbound_net > 0 ? "+" : ""}
                  {number.format(row.outbound_net)}
                </td>
                <td
                  className={`px-3 font-semibold ${row.inbound_net < 0 ? "text-red-300" : "text-mint"}`}
                >
                  {row.inbound_net > 0 ? "+" : ""}
                  {number.format(row.inbound_net)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-xs leading-relaxed text-dim">
        {analytics.data.unresolved_note}
      </p>
      <div className="mt-8 grid gap-6 xl:grid-cols-2">
        <FollowEventTable title="Following activity" events={events.filter(event=>event.direction==='outbound')} handles={handles} onAccount={onAccount}/>
        <FollowEventTable title="Follower activity" events={events.filter(event=>event.direction==='inbound')} handles={handles} onAccount={onAccount}/>
      </div>
    </>
  );
}
