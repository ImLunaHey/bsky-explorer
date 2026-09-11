import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
  ExternalLink,
  Image as ImageIcon,
  Search,
  Wifi,
  WifiOff,
} from "lucide-react";
import { api } from "./api";
import { useIdentityMap } from "./identities";
import type { LiveConnection } from "./live";
import type { FirehoseEvent, FirehoseFeed, RecordMediaResponse } from "./types";
import { HlsVideo } from "./HlsVideo";

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
  return (
    <div
      ref={host}
      className="grid h-20 w-28 shrink-0 place-items-center overflow-hidden rounded-lg border border-white/10 bg-white/[.025]"
    >
      {asset?.kind === "image" ? (
        <img
          src={asset.url}
          alt={asset.alt}
          loading="lazy"
          referrerPolicy="no-referrer"
          className="h-full w-full object-cover"
        />
      ) : asset?.kind === "video" ? (
        <HlsVideo
          src={asset.url}
          poster={asset.thumbnail_url}
          label={asset.alt || "Bluesky video preview"}
          className="h-full w-full"
        />
      ) : (
        <ImageIcon
          className={`size-4 ${media.isLoading ? "animate-pulse text-orange-300" : "text-white/15"}`}
        />
      )}
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
    refetchInterval: connection.data?.state === "connected" ? false : 3_000,
    refetchIntervalInBackground: true,
  });
  useEffect(() => {
    if (displayed === null && feed.data)
      setDisplayed([...feed.data.events].reverse());
  }, [displayed, feed.data]);
  const shown = displayed || EMPTY_EVENTS;
  const displayedIds = useMemo(
    () => new Set(shown.map((event) => event.event_id)),
    [shown],
  );
  const pending = useMemo(
    () =>
      [...(feed.data?.events || [])]
        .reverse()
        .filter((event) => !displayedIds.has(event.event_id)),
    [feed.data?.events, displayedIds],
  );
  const acceptPending = () => {
    setDisplayed((current) => [...pending, ...(current || [])].slice(0, 5_000));
    setPagination((value) => ({ ...value, pageIndex: 0 }));
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
            <button
              type="button"
              onClick={() => onAccount(did)}
              className="block min-w-0 max-w-60 text-left text-xs text-orange-300 hover:text-orange-200"
            >
              <b className="block break-all font-medium">
                {handle ? `@${handle}` : did}
              </b>
              {handle ? (
                <span className="block break-all font-mono text-[9px] text-white/35">
                  {did}
                </span>
              ) : (
                <span className="block text-[9px] text-dim">resolving…</span>
              )}
            </button>
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
          return (
            <div className="flex min-w-0 gap-3">
              <div className="min-w-0 flex-1">
                {event.text ? (
                  <p className="line-clamp-3 text-sm leading-relaxed text-white/80">
                    {event.text}
                  </p>
                ) : (
                  <p className="truncate font-mono text-[10px] text-white/40">
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
        },
      }),
      columnHelper.display({
        id: "actions",
        header: "",
        cell: (info) => {
          const uri = recordUri(info.row.original);
          return uri ? (
            <button
              type="button"
              onClick={() => onRecord(uri)}
              className="inline-flex items-center gap-1 whitespace-nowrap text-[11px] text-orange-300 hover:text-orange-200"
            >
              Inspect <ExternalLink className="size-3" />
            </button>
          ) : null;
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
      <section className="panel overflow-hidden">
        <div className="flex flex-col gap-4 border-b border-white/10 p-5 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <h2 className="font-semibold">Live firehose</h2>
              <span
                className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-wide ${live ? "bg-orange-400/10 text-orange-300" : "bg-red-300/10 text-red-300"}`}
              >
                {live ? (
                  <Wifi className="size-3" />
                ) : (
                  <WifiOff className="size-3" />
                )}
                {live ? "Streaming" : "Connecting"}
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
            {pending.length > 0 && (
              <button
                onClick={acceptPending}
                className="rounded-lg bg-orange-400 px-3 py-2 font-semibold text-black hover:bg-orange-300"
              >
                Show {pending.length.toLocaleString()} new
              </button>
            )}
          </div>
        </div>
        <div className="flex flex-col gap-3 border-b border-white/10 bg-white/[.015] p-4 sm:flex-row">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2.5 size-4 text-white/40" />
            <input
              value={globalFilter}
              onChange={(event) => setGlobalFilter(event.target.value)}
              placeholder="Filter text, DID, URL, or domain…"
              className="w-full rounded-lg border border-white/10 bg-black py-2 pl-9 pr-3 text-sm outline-none focus:border-orange-400/60"
            />
          </div>
          <select
            value={(table.getColumn("kind")?.getFilterValue() as string) || ""}
            onChange={(event) =>
              table
                .getColumn("kind")
                ?.setFilterValue(event.target.value || undefined)
            }
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
          <table className="w-full table-fixed border-collapse">
            <thead className="sticky top-0 z-10 bg-black">
              <tr>
                {table.getHeaderGroups()[0]?.headers.map((header) => (
                  <th
                    key={header.id}
                    style={{
                      width:
                        header.id === "time"
                          ? "7rem"
                          : header.id === "actor_did"
                            ? "13rem"
                            : header.id === "kind"
                              ? "6rem"
                              : header.id === "actions"
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
                  <td colSpan={5}>
                    <Empty text="Connecting to the live buffer…" />
                  </td>
                </tr>
              ) : table.getRowModel().rows.length === 0 ? (
                <tr>
                  <td colSpan={5}>
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
                    className="h-28 border-b border-white/[.07] align-middle hover:bg-white/[.02]"
                  >
                    {row.getVisibleCells().map((cell) => (
                      <td key={cell.id} className="overflow-hidden px-4 py-3">
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
