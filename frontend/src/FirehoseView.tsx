import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  type ColumnFiltersState,
  type PaginationState,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import {
  Activity,
  ChevronLeft,
  ChevronRight,
  Copy,
  Pause,
  Play,
  Search,
  Wifi,
  WifiOff,
} from "lucide-react";
import { api } from "./api";
import { IdentityLink } from "./identities";
import { appFor, apps } from './discovery';
import { eventAction, eventTarget } from './eventPresentation';
import { CoverageNote } from './CoverageNote';
import { FIREHOSE_PAUSE_EVENT, type LiveConnection } from "./live";
import type { FirehoseEvent, FirehoseFeed, RecordMediaResponse } from "./types";

const recordUri = (event: FirehoseEvent) =>
  event.at_uri?.startsWith("at://")
    ? event.at_uri
    : undefined;
const tones: Record<string, string> = {
  post: "bg-sky-400/10 text-sky-300",
  reply: "bg-sky-400/10 text-sky-300",
  like: "bg-pink-300/10 text-pink-300",
  repost: "bg-violet-300/10 text-violet-300",
  follow: "bg-sky-300/10 text-sky-300",
  unfollow: "bg-amber-300/10 text-amber-300",
  delete: "bg-red-300/10 text-red-300",
  quote: "bg-sky-300/10 text-sky-300",
};

function MediaPreview({ uri }: { uri: string }) {
  const host = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const node = host.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "120px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const media = useQuery({
    queryKey: ["record-media", uri],
    queryFn: () =>
      api<RecordMediaResponse>(
        `/api/records/media?uri=${encodeURIComponent(uri)}`,
      ),
    enabled: visible,
    staleTime: 300_000,
    gcTime: 900_000,
    retry: 0,
  });
  const asset = media.data?.media[0];
  if (!asset)
    return <div ref={host} aria-hidden="true" className="w-px self-stretch" />;
  return (
    <div
      ref={host}
      className="grid h-14 w-20 shrink-0 place-items-center overflow-hidden rounded-md border border-white/10 bg-white/[.025]"
    >
      {asset?.kind === "image" ? (
        <img
          src={asset.url}
          alt={asset.alt}
          loading="lazy"
          referrerPolicy="no-referrer"
          className="h-full w-full object-cover"
        />
      ) : asset?.kind === "video" && asset.thumbnail_url ? (
        <img
          src={asset.thumbnail_url}
          alt={asset.alt || "Bluesky video preview"}
          loading="lazy"
          referrerPolicy="no-referrer"
          className="h-full w-full object-cover"
        />
      ) : null}
    </div>
  );
}

const columnHelper = createColumnHelper<FirehoseEvent>();
const EMPTY_EVENTS: FirehoseEvent[] = [];

export function FirehoseView({
  onAccount,
  onRecord,
}: {
  onAccount: (did: string) => void;
  onRecord: (uri: string) => void;
}) {
  const [displayed, setDisplayed] = useState<FirehoseEvent[] | null>(null);
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: paused }));
    return () => {
      if (paused)
        window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: false }));
    };
  }, [paused]);
  const [sorting, setSorting] = useState<SortingState>([
    { id: "time", desc: true },
  ]);
  const search=useSearch({strict:false}) as {q?:string;kind?:string;account?:string;app?:string;collection?:string};
  const navigate=useNavigate();
  const updateSearch=(patch:Partial<typeof search>)=>void navigate({to:'/firehose',search:{...search,...patch},resetScroll:false});
  const filters=useMemo<ColumnFiltersState>(()=>search.kind?[{id:'kind',value:search.kind}]:[],[search.kind]);
  const globalFilter=search.q||'';
  const setGlobalFilter=(q:string)=>updateSearch({q:q||undefined});
  const setFilters=(update:ColumnFiltersState|((value:ColumnFiltersState)=>ColumnFiltersState))=>{const next=typeof update==='function'?update(filters):update;updateSearch({kind:next.find(filter=>filter.id==='kind')?.value as string|undefined})};
  const buffered=useQuery<{count:number;dropped:number}>({queryKey:['firehose-buffer'],queryFn:async()=>({count:0,dropped:0}),enabled:false});
  const [pagination, setPagination] = useState<PaginationState>({
    pageIndex: 0,
    pageSize: 25,
  });
  const connection = useQuery<LiveConnection>({
    queryKey: ["live-connection"],
    queryFn: async () => ({ state: "connecting" }),
    enabled: false,
  });
  const feed = useQuery({
    queryKey: ["firehose-feed"],
    queryFn: () => api<FirehoseFeed>("/api/firehose?limit=500"),
    refetchInterval:
      paused || connection.data?.state === "connected" ? false : 3_000,
    refetchIntervalInBackground: true,
  });
  const liveEvents = useMemo(
    () => [...(feed.data?.events || EMPTY_EVENTS)].reverse(),
    [feed.data?.events],
  );
  const shown = paused ? displayed || EMPTY_EVENTS : liveEvents;
  const togglePaused = () => {
    if (paused) {
      setDisplayed(null);
      setPagination((value) => ({ ...value, pageIndex: 0 }));
      setPaused(false);
    } else {
      setDisplayed(liveEvents);
      setPaused(true);
    }
  };
  const kinds = useMemo(
    () => [...new Set(shown.map((event) => event.kind))].sort(),
    [shown],
  );
  const columns = useMemo(
    () => [
      columnHelper.accessor("time_us", {
        id: "time",
        header: "Time",
        cell: (info) => (
          <time className="whitespace-nowrap text-[11px] tabular-nums text-white/45">
            {new Date(info.getValue() / 1_000).toLocaleTimeString()}
          </time>
        ),
      }),
      columnHelper.accessor("actor_did", {
        header: "Account",
        cell: (info) => {
          const did = info.getValue();
          return (
            <div className="group/account flex min-w-0 max-w-60 items-start gap-1">
              <IdentityLink did={did} onSelect={onAccount} compact className="flex-1 text-xs" />
              <button type="button" onClick={() => void navigator.clipboard.writeText(did)} title={`Copy full DID: ${did}`} aria-label={`Copy full DID ${did}`} className="shrink-0 rounded p-1 text-white/25 opacity-60 hover:bg-white/5 hover:text-sky-300 focus:opacity-100 sm:opacity-0 sm:group-hover/account:opacity-100"><Copy className="size-3" /></button>
            </div>
          );
        },
      }),
      columnHelper.accessor("kind", {
        header: "Type",
        filterFn: "equals",
        cell: (info) => (
          <span
            className={`rounded px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider ${tones[info.getValue()] || "bg-white/5 text-white/45"}`}
          >
            {info.getValue()}
          </span>
        ),
      }),
      columnHelper.display({
        id: "event",
        header: "Event",
        cell: (info) => {
          const event = info.row.original,
            uri = recordUri(event);
          const target=eventTarget(event);
          return <div className="space-y-2 text-xs">
            <p className="flex flex-wrap items-baseline gap-1"><span>{eventAction(event)}</span>{target&&<><span className="text-dim">{['follow','unfollow','block','unblock'].includes(event.kind)?'':'by'}</span><IdentityLink did={target} inline /></>}</p>
            {event.text&&<p className="line-clamp-2 whitespace-pre-wrap text-white/80">{event.text}</p>}
            {uri&&<button onClick={()=>onRecord(uri)} className="text-cyan">Open record →</button>}
            {uri?.includes('/app.bsky.feed.post/')&&<MediaPreview uri={uri}/>}
            <details className="text-[10px] text-dim"><summary className="cursor-pointer">Record details</summary><dl className="mt-2 space-y-1 break-all font-mono"><dt>Collection</dt><dd>{event.collection||'—'}</dd><dt>Record</dt><dd>{event.at_uri||'—'}</dd><dt>Target</dt><dd>{event.target||event.target_did||'—'}</dd><dt>Event ID</dt><dd>{event.event_id}</dd></dl></details>
          </div>;
        },
      }),
    ],
    [onAccount, onRecord],
  );
  const filteredEvents=useMemo(()=>shown.filter(event=>(!search.account||event.actor_did.includes(search.account)||event.target_did?.includes(search.account))&&(!search.collection||event.collection===search.collection)&&(!search.app||(search.app==='bsky'?event.collection.startsWith('app.bsky.'):appFor(event.collection)?.id===search.app))),[shown,search.account,search.collection,search.app]);
  const collections=useMemo(()=>[...new Set(shown.map(event=>event.collection).filter(Boolean))].sort(),[shown]);
  const table = useReactTable({
    data: filteredEvents,
    columns,
    state: { sorting, columnFilters: filters, globalFilter, pagination },
    onSortingChange: setSorting,
    onColumnFiltersChange: setFilters,
    onGlobalFilterChange: value=>setGlobalFilter(typeof value==='function'?value(globalFilter):value),
    onPaginationChange: setPagination,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    globalFilterFn: (row, _columnId, value) => {
      const event = row.original,
        needle = String(value).toLocaleLowerCase();
      return [
        event.actor_did,
        event.kind,
        event.collection,
        event.text,
        event.at_uri,
        event.target,
        ...(event.urls || []),
        ...(event.domains || []),
      ].some((item) => item?.toLocaleLowerCase().includes(needle));
    },
  });
  const live = connection.data?.state === "connected";
  return (
    <div className="space-y-5">
      <CoverageNote kind="live">Filters apply to the buffered events on this page, not the complete archive. Pause freezes rows; account names and the waiting-event count keep updating.</CoverageNote>
      <div className="flex w-fit rounded-lg border border-white/10 bg-white/[.02] p-1 text-xs">
        <Link to="/firehose" className="rounded-md bg-sky-400 px-3 py-1.5 font-semibold text-black">Events</Link>
        <Link to="/firehose/images" className="rounded-md px-3 py-1.5 text-white/55 hover:text-sky-300">Images</Link>
      </div>
      <section className="panel overflow-hidden">
        <div className="flex flex-col gap-4 border-b border-white/10 p-5 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <h2 className="font-semibold">Live firehose</h2>
              <span
                className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-wide ${paused ? "bg-amber-400/10 text-amber-300" : live ? "bg-sky-400/10 text-sky-300" : "bg-red-300/10 text-red-300"}`}
              >
                {live ? (
                  <Wifi className="size-3" />
                ) : (
                  <WifiOff className="size-3" />
                )}
                {paused ? "Paused" : live ? "Streaming" : "Connecting"}
              </span>
            </div>
            <p className="mt-1 text-xs text-white/45">
              Incoming events are buffered so the page never moves while you
              read it.
            </p>
          </div>
          <div className="flex items-center gap-3 text-xs text-white/45">
            {paused&&<span role="status" className="text-cyan">{(buffered.data?.count||0).toLocaleString()}{buffered.data?.dropped?'+':''} new events waiting{buffered.data?.dropped?' · buffer capped at 5,000':''}</span>}
            <span>
              {table.getFilteredRowModel().rows.length.toLocaleString()} events
              cached
            </span>
            <button type="button" onClick={togglePaused} className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 px-3 py-2 font-medium text-white/70 hover:border-sky-400/40 hover:text-sky-300">
              {paused ? <Play className="size-3" /> : <Pause className="size-3" />}
              {paused ? "Resume" : "Pause"}
            </button>
          </div>
        </div>
        <div className="grid gap-3 border-b border-white/10 bg-white/[.015] p-4 sm:grid-cols-2 lg:grid-cols-3">
          <input aria-label="Filter account DID" placeholder="Account DID (actor or target)" value={search.account||''} onChange={e=>{updateSearch({account:e.target.value||undefined});setPagination(value=>({...value,pageIndex:0}))}} className="border border-line bg-black px-3 py-2 text-xs"/>
          <select aria-label="Filter app" value={search.app||''} onChange={e=>{updateSearch({app:e.target.value||undefined});setPagination(value=>({...value,pageIndex:0}))}} className="border border-line bg-black px-3 py-2 text-xs"><option value="">All apps</option><option value="bsky">Bluesky</option>{apps.map(app=><option key={app.id} value={app.id}>{app.name}</option>)}</select>
          <select aria-label="Filter collection" value={search.collection||''} onChange={e=>{updateSearch({collection:e.target.value||undefined});setPagination(value=>({...value,pageIndex:0}))}} className="min-w-0 border border-line bg-black px-3 py-2 text-xs"><option value="">All collections</option>{[...new Set([...collections,...(search.collection?[search.collection]:[])])].map(collection=><option key={collection}>{collection}</option>)}</select>
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2.5 size-4 text-white/40" />
            <input
              value={globalFilter}
              onChange={(event) => { setGlobalFilter(event.target.value); setPagination((value) => ({ ...value, pageIndex: 0 })); }}
              placeholder="Filter collection, text, DID, URL…"
              className="w-full rounded-lg border border-white/10 bg-black py-2 pl-9 pr-3 text-sm outline-none focus:border-sky-400/60"
            />
          </div>
          <select
            value={(table.getColumn("kind")?.getFilterValue() as string) || ""}
            onChange={(event) => {
              table
                .getColumn("kind")
                ?.setFilterValue(event.target.value || undefined);
              setPagination((value) => ({ ...value, pageIndex: 0 }));
            }}
            className="rounded-lg border border-white/10 bg-black px-3 py-2 text-sm"
          >
            <option value="">All event types</option>
            {kinds.map((kind) => (
              <option key={kind} value={kind}>
                {kind}
              </option>
            ))}
          </select>
        </div>
        <div className="min-h-96 overflow-x-auto">
          <table className="w-full min-w-[52rem] table-fixed border-collapse">
            <thead className="sticky top-0 z-10 bg-black">
              <tr>
                {table.getHeaderGroups()[0]?.headers.map((header) => (
                  <th
                    key={header.id}
                    style={{
                      width:
                        header.id === "time"
                          ? "6.5rem"
                          : header.id === "actor_did"
                            ? "15rem"
                            : header.id === "kind"
                              ? "5rem"
                              : undefined,
                    }}
                    className="border-b border-white/10 px-4 py-3 text-left text-[10px] uppercase tracking-wider text-white/40"
                  >
                    <button
                      className={
                        header.column.getCanSort()
                          ? "cursor-pointer select-none hover:text-white"
                          : ""
                      }
                      onClick={header.column.getToggleSortingHandler()}
                    >
                      {flexRender(
                        header.column.columnDef.header,
                        header.getContext(),
                      )}
                      {header.column.getIsSorted() === "asc"
                        ? " ↑"
                        : header.column.getIsSorted() === "desc"
                          ? " ↓"
                          : ""}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {feed.isLoading && !displayed ? (
                <tr>
                  <td colSpan={4}>
                    <Empty text="Connecting to the live buffer…" />
                  </td>
                </tr>
              ) : table.getRowModel().rows.length === 0 ? (
                <tr>
                  <td colSpan={4}>
                    <Empty
                      text={
                        shown.length
                          ? "No events match these filters."
                          : "Waiting for live activity…"
                      }
                    />
                  </td>
                </tr>
              ) : (
                table.getRowModel().rows.map((row) => (
                  <tr
                    key={row.original.event_id}
                    className="border-b border-white/[.07] align-middle hover:bg-white/[.02]"
                  >
                    {row.getVisibleCells().map((cell) => (
                      <td key={cell.id} className="overflow-hidden px-3 py-2.5">
                        {flexRender(
                          cell.column.columnDef.cell,
                          cell.getContext(),
                        )}
                      </td>
                    ))}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-white/10 px-5 py-3 text-xs text-white/45">
          <span>
            Page {table.getState().pagination.pageIndex + 1} of{" "}
            {Math.max(1, table.getPageCount())} · up to 5,000 events retained in
            this browser
          </span>
          <div className="flex items-center gap-2">
            <label>
              Rows{" "}
              <select
                value={pagination.pageSize}
                onChange={(event) =>
                  table.setPageSize(Number(event.target.value))
                }
                className="ml-1 rounded border border-white/10 bg-black px-2 py-1"
              >
                {[10, 25, 50].map((size) => (
                  <option key={size}>{size}</option>
                ))}
              </select>
            </label>
            <button
              aria-label="Previous page"
              onClick={() => table.previousPage()}
              disabled={!table.getCanPreviousPage()}
              className="rounded border border-white/10 p-1.5 disabled:opacity-30"
            >
              <ChevronLeft className="size-4" />
            </button>
            <button
              aria-label="Next page"
              onClick={() => table.nextPage()}
              disabled={!table.getCanNextPage()}
              className="rounded border border-white/10 p-1.5 disabled:opacity-30"
            >
              <ChevronRight className="size-4" />
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div className="grid min-h-96 place-items-center p-8 text-sm text-white/45">
      <div className="text-center">
        <Activity className="mx-auto mb-3 size-6 opacity-50" />
        {text}
      </div>
    </div>
  );
}
