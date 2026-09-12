import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
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
import { useIdentityMap } from "./identities";
import { FIREHOSE_PAUSE_EVENT, type LiveConnection } from "./live";
import type { FirehoseEvent, FirehoseFeed, RecordMediaResponse } from "./types";

const recordUri = (event: FirehoseEvent) =>
  ["post", "reply", "quote"].includes(event.kind) &&
  event.at_uri?.includes("/app.bsky.feed.post/")
    ? event.at_uri
    : undefined;
const tones: Record<string, string> = {
  post: "bg-orange-400/10 text-orange-300",
  reply: "bg-orange-400/10 text-orange-300",
  like: "bg-pink-300/10 text-pink-300",
  repost: "bg-violet-300/10 text-violet-300",
  follow: "bg-orange-300/10 text-orange-300",
  unfollow: "bg-amber-300/10 text-amber-300",
  delete: "bg-red-300/10 text-red-300",
  quote: "bg-orange-300/10 text-orange-300",
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
  const [filters, setFilters] = useState<ColumnFiltersState>([]);
  const [globalFilter, setGlobalFilter] = useState("");
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
  const roughPage = shown.slice(
    pagination.pageIndex * pagination.pageSize,
    (pagination.pageIndex + 1) * pagination.pageSize,
  );
  const handles = useIdentityMap(roughPage.map((event) => event.actor_did));
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
          const did = info.getValue(),
            handle = handles.get(did);
          return (
            <div className="group/account flex min-w-0 max-w-60 items-start gap-1">
              <button
                type="button"
                onClick={() => onAccount(did)}
                title={handle ? `@${handle}\n${did}` : did}
                className="min-w-0 flex-1 text-left text-xs text-orange-300 hover:text-orange-200"
              >
              <b className={`block font-medium ${handle ? "truncate" : "break-all font-mono text-[10px] leading-tight"}`}>
                {handle ? `@${handle}` : did}
              </b>
              {handle ? (
                <span className="block break-all font-mono text-[9px] leading-tight text-white/35">
                  {did}
                </span>
              ) : (
                <span className="block pt-0.5 text-[9px] text-dim">resolving…</span>
              )}
              </button>
              <button type="button" onClick={() => void navigator.clipboard.writeText(did)} title={`Copy full DID: ${did}`} aria-label={`Copy full DID ${did}`} className="shrink-0 rounded p-1 text-white/25 opacity-60 hover:bg-white/5 hover:text-orange-300 focus:opacity-100 sm:opacity-0 sm:group-hover/account:opacity-100"><Copy className="size-3" /></button>
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
          const content = (
            <div className="flex min-w-0 items-center gap-2 text-left">
              <div className="min-w-0 flex-1">
                {event.text ? (
                  <p className="line-clamp-2 text-xs leading-relaxed text-white/80" title={event.text}>
                    {event.text}
                  </p>
                ) : (
                  <p className="truncate font-mono text-[10px] text-white/40" title={event.target || event.at_uri || event.event_id}>
                    {event.target || event.at_uri || event.event_id}
                  </p>
                )}
                <div className="mt-1 flex flex-wrap gap-1">
                  {event.domains?.map((domain) => (
                    <span
                      key={domain}
                      className="rounded bg-orange-400/5 px-1.5 py-0.5 text-[10px] text-orange-300"
                    >
                      {domain}
                    </span>
                  ))}
                </div>
              </div>
              {uri && <MediaPreview uri={uri} />}
            </div>
          );
          return uri ? (
            <button
              type="button"
              onClick={() => onRecord(uri)}
              title="Open record detail"
              className="block w-full rounded-md outline-none hover:bg-white/[.025] focus-visible:ring-1 focus-visible:ring-orange-400/70"
            >
              {content}
            </button>
          ) : content;
        },
      }),
    ],
    [handles, onAccount, onRecord],
  );
  const table = useReactTable({
    data: shown,
    columns,
    state: { sorting, columnFilters: filters, globalFilter, pagination },
    onSortingChange: setSorting,
    onColumnFiltersChange: setFilters,
    onGlobalFilterChange: setGlobalFilter,
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
      <div className="flex w-fit rounded-lg border border-white/10 bg-white/[.02] p-1 text-xs">
        <Link to="/firehose" className="rounded-md bg-orange-400 px-3 py-1.5 font-semibold text-black">Events</Link>
        <Link to="/firehose/images" className="rounded-md px-3 py-1.5 text-white/55 hover:text-orange-300">Images</Link>
      </div>
      <section className="panel overflow-hidden">
        <div className="flex flex-col gap-4 border-b border-white/10 p-5 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <h2 className="font-semibold">Live firehose</h2>
              <span
                className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-wide ${paused ? "bg-amber-400/10 text-amber-300" : live ? "bg-orange-400/10 text-orange-300" : "bg-red-300/10 text-red-300"}`}
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
            <span>
              {table.getFilteredRowModel().rows.length.toLocaleString()} events
              cached
            </span>
            <button type="button" onClick={togglePaused} className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 px-3 py-2 font-medium text-white/70 hover:border-orange-400/40 hover:text-orange-300">
              {paused ? <Play className="size-3" /> : <Pause className="size-3" />}
              {paused ? "Resume" : "Pause"}
            </button>
          </div>
        </div>
        <div className="flex flex-col gap-3 border-b border-white/10 bg-white/[.015] p-4 sm:flex-row">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2.5 size-4 text-white/40" />
            <input
              value={globalFilter}
              onChange={(event) => { setGlobalFilter(event.target.value); setPagination((value) => ({ ...value, pageIndex: 0 })); }}
              placeholder="Filter text, DID, URL, or domain…"
              className="w-full rounded-lg border border-white/10 bg-black py-2 pl-9 pr-3 text-sm outline-none focus:border-orange-400/60"
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
