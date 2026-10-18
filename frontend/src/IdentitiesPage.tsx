import { useMemo, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { Chart } from "@tanstack/charts/react";
import { defineChart, lineY } from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { tooltip } from "@tanstack/charts/tooltip";
import { ArrowDownUp, ChevronLeft, ChevronRight, Search } from "lucide-react";
import { api } from "./api";
import type {
  IdentityQueueAnalytics,
  IdentityQueueJob,
  IdentityQueueResponse,
  IdentityQueueStatus,
} from "./types";

const PAGE_SIZE = 25;
const compact = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});
const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
const duration = (milliseconds?: number | null) => {
  if (milliseconds == null || !Number.isFinite(milliseconds)) return "—";
  if (milliseconds < 1_000) return `${Math.round(milliseconds)} ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)} s`;
  if (milliseconds < 3_600_000)
    return `${(milliseconds / 60_000).toFixed(1)} min`;
  return `${(milliseconds / 3_600_000).toFixed(1)} h`;
};
const localTime = (value: number | string) => {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "—";
  const today = new Date();
  const sameDay =
    date.getFullYear() === today.getFullYear() &&
    date.getMonth() === today.getMonth() &&
    date.getDate() === today.getDate();
  return sameDay
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString([], {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
};
const relative = (value?: string | null) => {
  if (!value) return "—";
  const delta = Date.now() - new Date(value).getTime();
  if (!Number.isFinite(delta)) return "—";
  const future = delta < 0;
  const seconds = Math.abs(delta) / 1_000;
  const amount =
    seconds < 60
      ? `${Math.round(seconds)}s`
      : seconds < 3_600
        ? `${Math.round(seconds / 60)}m`
        : seconds < 86_400
          ? `${Math.round(seconds / 3_600)}h`
          : `${Math.round(seconds / 86_400)}d`;
  return future ? `in ${amount}` : `${amount} ago`;
};
const statusTone: Record<IdentityQueueStatus, string> = {
  queued: "border-white/15 text-white/70",
  running: "border-sky-400/40 bg-sky-500/10 text-sky-300",
  retrying: "border-amber-400/40 bg-amber-500/10 text-amber-300",
  resolved: "border-emerald-400/30 bg-emerald-500/10 text-emerald-300",
  negative: "border-zinc-400/30 text-zinc-300",
  dead_letter: "border-red-400/40 bg-red-500/10 text-red-300",
};

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="rounded-xl border border-line bg-panel/70 p-4">
      <p className="text-[10px] uppercase tracking-[.14em] text-dim">{label}</p>
      <p className="mt-2 text-2xl font-semibold tabular-nums">{value}</p>
      {note && <p className="mt-1 text-xs text-dim">{note}</p>}
    </div>
  );
}

function QueueChart({ analytics }: { analytics: IdentityQueueAnalytics }) {
  const rows = useMemo(
    () =>
      analytics.series.map((sample) => ({
        ...sample,
        time: new Date(sample.bucket).getTime(),
      })),
    [analytics.series],
  );
  const throughput = useMemo(
    () =>
      defineChart({
        marks: [
          lineY(rows, { id: "Resolved", x: "time", y: "resolved", stroke: "#58b8ff", points: true }),
          lineY(rows, { id: "Negative", x: "time", y: "negative", stroke: "#ffbf69", points: true }),
          lineY(rows, { id: "Failures", x: "time", y: "failures", stroke: "#ef4444", points: true }),
        ],
        scales: {
          x: { scale: scaleLinear, axis: { label: "Local time", ticks: { format: (value: number) => localTime(value) } } },
          y: { scale: scaleLinear, axis: { label: "Completed jobs", ticks: { format: (value: number) => compact.format(value) } } },
        },
        focus: "group-x",
        maxFocusDistance: Number.POSITIVE_INFINITY,
        tooltip: {
          use: tooltip,
          items: [
            { channel: "x", label: "Bucket", text: (item) => localTime(Number(item.xValue)) },
            { channel: "y", label: "Jobs", text: (item) => compact.format(Number(item.yValue)) },
          ],
        },
      }),
    [rows],
  );
  const latencyRows = useMemo(
    () => rows.filter((sample) => sample.avg_duration_ms != null),
    [rows],
  );
  const latency = useMemo(
    () =>
      defineChart({
        marks: [lineY(latencyRows, { id: "Average latency", x: "time", y: "avg_duration_ms", stroke: "#58b8ff", points: true })],
        scales: {
          x: { scale: scaleLinear, axis: { label: "Local time", ticks: { format: (value: number) => localTime(value) } } },
          y: { scale: scaleLinear, axis: { label: "Resolution latency", ticks: { format: (value: number) => duration(value) } } },
        },
        focus: "nearest-x",
        maxFocusDistance: Number.POSITIVE_INFINITY,
        tooltip: {
          use: tooltip,
          items: [
            { channel: "x", label: "Bucket", text: (item) => localTime(Number(item.xValue)) },
            { channel: "y", label: "Average latency", text: (item) => duration(Number(item.yValue)) },
          ],
        },
      }),
    [latencyRows],
  );
  return (
    <div className="grid gap-5 lg:grid-cols-2">
      <section className="rounded-xl border border-line bg-panel/70 p-5">
        <h2 className="font-semibold">Resolution outcomes</h2>
        <p className="mt-1 text-xs text-dim">Completed identity work over time</p>
        <div className="mt-5">
          {rows.length > 1 ? <Chart definition={throughput} height={210} ariaLabel="Identity resolution outcomes" /> : <p className="grid h-52 place-items-center text-sm text-dim">Collecting queue history…</p>}
        </div>
        <div className="mt-2 flex justify-center gap-5 text-[10px] uppercase tracking-wide text-dim">
          <span className="text-sky-400">● Resolved</span><span className="text-sky-200">● Negative</span><span className="text-red-400">● Failures</span>
        </div>
      </section>
      <section className="rounded-xl border border-line bg-panel/70 p-5">
        <h2 className="font-semibold">Resolution latency</h2>
        <p className="mt-1 text-xs text-dim">Average completed-job duration per bucket</p>
        <div className="mt-5">
          {latencyRows.length > 1 ? <Chart definition={latency} height={210} ariaLabel="Identity resolution latency" /> : <p className="grid h-52 place-items-center text-sm text-dim">Collecting latency history…</p>}
        </div>
      </section>
    </div>
  );
}

const helper = createColumnHelper<IdentityQueueJob>();

export function IdentitiesPage() {
  const [page, setPage] = useState(0);
  const [status, setStatus] = useState<IdentityQueueStatus | "all">("all");
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [sorting, setSorting] = useState<SortingState>([{ id: "updated_at", desc: true }]);
  const sort = sorting[0]?.id || "updated_at";
  const order = sorting[0]?.desc === false ? "asc" : "desc";
  const params = new URLSearchParams({
    limit: String(PAGE_SIZE),
    offset: String(page * PAGE_SIZE),
    sort,
    order,
    hours: "24",
    bucket_minutes: "15",
  });
  if (status !== "all") params.set("status", status);
  if (query) params.set("q", query);
  const queue = useQuery({
    queryKey: ["identity-queue-operations", page, status, query, sort, order],
    queryFn: () => api<IdentityQueueResponse>(`/api/identity-queue?${params}`),
    placeholderData: keepPreviousData,
    refetchInterval: (current) => {
      const summary = current.state.data?.summary;
      return summary && summary.queued + summary.running + summary.retrying > 0 ? 10_000 : false;
    },
  });
  const columns = useMemo(
    () => [
      helper.accessor("did", {
        header: "Account",
        enableSorting: false,
        cell: ({ row }) => (
          <div className="max-w-80">
            <Link to="/accounts/$did" params={{ did: row.original.did }} className="block hover:text-sky-300">
              {row.original.handle && <b className="block truncate text-sm">@{row.original.handle}</b>}
              <span className="block break-all font-mono text-[10px] text-dim">{row.original.did}</span>
            </Link>
            <Link to="/identities/$jobId" params={{ jobId: row.original.id }} className="mt-1 inline-block font-mono text-[9px] text-sky-400 hover:text-sky-300">{row.original.id}</Link>
          </div>
        ),
      }),
      helper.accessor("status", {
        header: "Status",
        enableSorting: false,
        cell: ({ getValue }) => <span className={`inline-flex rounded-full border px-2 py-1 text-[10px] uppercase tracking-wide ${statusTone[getValue()]}`}>{getValue().replace("_", " ")}</span>,
      }),
      helper.accessor("attempts", { header: "Attempts", cell: ({ row }) => <span className="tabular-nums">{row.original.attempts} / {row.original.max_attempts}</span> }),
      helper.accessor("created_at", { header: "Queued", cell: ({ getValue }) => <time title={new Date(getValue()).toLocaleString()}>{relative(getValue())}</time> }),
      helper.accessor("started_at", { header: "Started", enableSorting: false, cell: ({ getValue }) => <time title={getValue() ? new Date(getValue()!).toLocaleString() : undefined}>{relative(getValue())}</time> }),
      helper.accessor("completed_at", { header: "Finished", enableSorting: false, cell: ({ getValue }) => <time title={getValue() ? new Date(getValue()!).toLocaleString() : undefined}>{relative(getValue())}</time> }),
      helper.accessor("duration_ms", { header: "Duration", cell: ({ getValue }) => duration(getValue()) }),
      helper.accessor("updated_at", { header: "Updated", cell: ({ getValue }) => <time title={new Date(getValue()).toLocaleString()}>{relative(getValue())}</time> }),
      helper.accessor("last_error", { header: "Last error", enableSorting: false, cell: ({ getValue }) => <span className="block max-w-72 truncate text-xs text-red-300" title={getValue() || undefined}>{getValue() || "—"}</span> }),
    ],
    [],
  );
  const table = useReactTable({
    data: queue.data?.jobs || [],
    columns,
    state: { sorting },
    onSortingChange: (next) => {
      setSorting(next);
      setPage(0);
    },
    manualSorting: true,
    getCoreRowModel: getCoreRowModel(),
  });
  const data = queue.data;
  const analytics = data?.analytics;
  return (
    <div className="space-y-5">
      <header>
        <p className="eyebrow">Operations</p>
        <h1 className="mt-2 text-2xl font-semibold">Identity resolution</h1>
        <p className="mt-2 max-w-3xl text-sm text-dim">Durable DID-to-handle enrichment. Existing work is retained permanently; ordinary firehose traffic does not add remote-resolution jobs.</p>
      </header>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-7">
        {(["total", "queued", "running", "retrying", "resolved", "negative", "dead_letter"] as const).map((key) => (
          <Stat key={key} label={key.replace("_", " ")} value={data ? compact.format(data.summary[key]) : "—"} />
        ))}
      </div>
      {analytics && (
        <>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Stat label="Last hour" value={compact.format(analytics.throughput_1h)} note={`${compact.format(analytics.throughput_5m)} in 5 minutes`} />
            <Stat label="Success rate" value={`${number.format(analytics.success_rate * 100)}%`} note={`${number.format(analytics.negative_rate * 100)}% negative`} />
            <Stat label="Median latency" value={duration(analytics.p50_duration_ms)} note={`p95 ${duration(analytics.p95_duration_ms)}`} />
            <Stat label="Backlog ETA" value={duration(analytics.backlog_eta_seconds == null ? null : analytics.backlog_eta_seconds * 1_000)} note={analytics.oldest_queued_age_seconds == null ? "No queued work" : `Oldest queued ${duration(analytics.oldest_queued_age_seconds * 1_000)}`} />
          </div>
          <QueueChart analytics={analytics} />
        </>
      )}
      <section className="rounded-xl border border-line bg-panel/70">
        <div className="flex flex-col gap-3 border-b border-line p-4 lg:flex-row lg:items-end lg:justify-between">
          <div><h2 className="font-semibold">Resolution jobs</h2><p className="mt-1 text-xs text-dim">Server-filtered and paginated queue history</p></div>
          <div className="flex flex-col gap-2 sm:flex-row">
            <form className="relative" onSubmit={(event) => { event.preventDefault(); setQuery(search.trim()); setPage(0); }}>
              <Search className="absolute left-3 top-2.5 size-4 text-dim" />
              <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="DID or handle" aria-label="Search identity jobs" className="h-9 w-full rounded-lg border border-line bg-black pl-9 pr-3 text-xs outline-none focus:border-sky-500 sm:w-64" />
            </form>
            <select value={status} onChange={(event) => { setStatus(event.target.value as IdentityQueueStatus | "all"); setPage(0); }} aria-label="Filter by identity status" className="h-9 rounded-lg border border-line bg-black px-3 text-xs">
              <option value="all">All statuses</option>
              {Object.keys(statusTone).map((value) => <option key={value} value={value}>{value.replace("_", " ")}</option>)}
            </select>
          </div>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full min-w-300 text-left text-xs">
            <thead className="border-b border-line text-[10px] uppercase tracking-wide text-dim"><tr>{table.getHeaderGroups()[0]?.headers.map((header) => <th key={header.id} className="whitespace-nowrap px-4 py-3"><button type="button" disabled={!header.column.getCanSort()} onClick={header.column.getToggleSortingHandler()} className="inline-flex items-center gap-1 disabled:cursor-default">{flexRender(header.column.columnDef.header, header.getContext())}{header.column.getCanSort() && <ArrowDownUp className="size-3" />}</button></th>)}</tr></thead>
            <tbody>{queue.isLoading ? <tr><td colSpan={columns.length} className="h-32 text-center text-dim">Loading identity jobs…</td></tr> : table.getRowModel().rows.length ? table.getRowModel().rows.map((row) => <tr key={row.id} className="border-b border-line/60 hover:bg-white/[.025]">{row.getVisibleCells().map((cell) => <td key={cell.id} className="px-4 py-3 align-top">{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>) : <tr><td colSpan={columns.length} className="h-32 text-center text-dim">No identity jobs match these filters.</td></tr>}</tbody>
          </table>
        </div>
        <footer className="flex items-center justify-between gap-4 p-4 text-xs text-dim">
          <span>{data ? `${data.total.toLocaleString()} jobs · page ${page + 1}` : "Loading…"}</span>
          <div className="flex gap-2"><button type="button" disabled={page === 0 || queue.isFetching} onClick={() => setPage((value) => Math.max(0, value - 1))} className="rounded-lg border border-line p-2 disabled:opacity-30" aria-label="Previous page"><ChevronLeft className="size-4" /></button><button type="button" disabled={!data?.has_more || queue.isFetching} onClick={() => setPage((value) => value + 1)} className="rounded-lg border border-line p-2 disabled:opacity-30" aria-label="Next page"><ChevronRight className="size-4" /></button></div>
        </footer>
      </section>
    </div>
  );
}

export function IdentityDetailPage({ jobId }: { jobId: string }) {
  const job = useQuery({
    queryKey: ["identity-queue-job", jobId],
    queryFn: () => api<IdentityQueueJob>(`/api/identity-queue/${encodeURIComponent(jobId)}`),
  });
  if (job.isLoading) return <p className="grid min-h-64 place-items-center text-sm text-dim">Loading identity job…</p>;
  if (!job.data) return <p className="grid min-h-64 place-items-center text-sm text-red-300">Identity job is unavailable.</p>;
  const item = job.data;
  const fields: [string, string][] = [
    ["Job", item.id], ["Status", item.status.replace("_", " ")], ["Attempts", `${item.attempts} / ${item.max_attempts}`],
    ["Queued", new Date(item.created_at).toLocaleString()], ["Started", item.started_at ? new Date(item.started_at).toLocaleString() : "—"],
    ["Finished", item.completed_at ? new Date(item.completed_at).toLocaleString() : "—"], ["Duration", duration(item.duration_ms)],
    ["Updated", new Date(item.updated_at).toLocaleString()], ["Next attempt", item.next_attempt_at ? new Date(item.next_attempt_at).toLocaleString() : "—"],
  ];
  return (
    <div className="mx-auto max-w-4xl space-y-5">

      <section className="rounded-xl border border-line bg-panel/70 p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="eyebrow">Identity job</p>
            {item.handle && <h1 className="mt-2 text-2xl font-semibold">@{item.handle}</h1>}
            <Link to="/accounts/$did" params={{ did: item.did }} className="mt-2 block break-all font-mono text-xs text-sky-300 hover:text-sky-200">{item.did}</Link>
          </div>
          <span className={`rounded-full border px-3 py-1 text-xs uppercase tracking-wide ${statusTone[item.status]}`}>{item.status.replace("_", " ")}</span>
        </div>
        <dl className="mt-6 grid gap-4 sm:grid-cols-2">
          {fields.map(([label, value]) => <div key={label} className="border-b border-line/60 pb-3"><dt className="text-[10px] uppercase tracking-wide text-dim">{label}</dt><dd className="mt-1 break-all font-mono text-sm">{value}</dd></div>)}
        </dl>
        {item.last_error && <div className="mt-5 rounded-lg border border-red-500/30 bg-red-500/5 p-4"><p className="text-[10px] uppercase tracking-wide text-red-300">Last error</p><p className="mt-2 whitespace-pre-wrap break-words font-mono text-xs text-red-200">{item.last_error}</p></div>}
      </section>
    </div>
  );
}
