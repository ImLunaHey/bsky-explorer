import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { Chart } from "@tanstack/charts/react";
import { defineChart, lineY } from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { tooltip } from "@tanstack/charts/tooltip";
import { ArrowDownUp, ChevronLeft, ChevronRight, Search } from "lucide-react";
import { api } from "./api";
import type { CoverageCollection, CoverageResponse } from "./types";

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
const bytes = (value?: number | null) => {
  if (value == null || !Number.isFinite(value)) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let amount = Math.max(0, value), unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit++; }
  return `${amount.toLocaleString(undefined, { maximumFractionDigits: unit ? 2 : 0 })} ${units[unit]}`;
};
const day = (value: number | string) => new Date(value).toLocaleDateString([], { month: "short", day: "numeric" });

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return <div className="panel p-4"><p className="eyebrow">{label}</p><p className="mt-2 text-2xl font-semibold tabular-nums">{value}</p>{note && <p className="mt-1 text-xs text-dim">{note}</p>}</div>;
}

function CoverageCharts({ data }: { data: CoverageResponse["daily"] }) {
  const rows = useMemo(() => data.map((item) => ({ ...item, time: new Date(`${item.date}T00:00:00`).getTime() })), [data]);
  const accounts = useMemo(() => defineChart({
    marks: [lineY(rows, { id: "Active accounts", x: "time", y: "active_accounts", stroke: "#58b8ff", points: true }), lineY(rows, { id: "New accounts", x: "time", y: "new_accounts", stroke: "#c4e6ff", points: true })],
    scales: { x: { scale: scaleLinear, axis: { label: "Local date", ticks: { format: (value: number) => day(value) } } }, y: { scale: scaleLinear, axis: { label: "Unique accounts", ticks: { format: (value: number) => compact.format(value) } } } },
    focus: "group-x", maxFocusDistance: Number.POSITIVE_INFINITY,
    tooltip: { use: tooltip, items: [{ channel: "x", label: "Date", text: (item) => new Date(Number(item.xValue)).toLocaleDateString() }, { channel: "y", label: "Accounts", text: (item) => Number(item.yValue).toLocaleString() }] },
  }), [rows]);
  const records = useMemo(() => defineChart({
    marks: [lineY(rows, { id: "Observed records", x: "time", y: "records", stroke: "#58b8ff", points: true })],
    scales: { x: { scale: scaleLinear, axis: { label: "Local date", ticks: { format: (value: number) => day(value) } } }, y: { scale: scaleLinear, axis: { label: "Records", ticks: { format: (value: number) => compact.format(value) } } } },
    focus: "nearest-x", maxFocusDistance: Number.POSITIVE_INFINITY,
    tooltip: { use: tooltip, items: [{ channel: "x", label: "Date", text: (item) => new Date(Number(item.xValue)).toLocaleDateString() }, { channel: "y", label: "Records", text: (item) => Number(item.yValue).toLocaleString() }] },
  }), [rows]);
  return <div className="grid gap-5 lg:grid-cols-2"><section className="panel p-5"><h2 className="font-semibold">Account coverage growth</h2><p className="mt-1 text-xs text-dim">Daily unique active and first-observed accounts</p><div className="mt-5">{rows.length > 1 ? <Chart definition={accounts} height={220} ariaLabel="Daily active and new observed accounts" /> : <Empty />}</div><div className="mt-2 flex justify-center gap-5 text-[10px] uppercase tracking-wide text-dim"><span className="text-sky-400">● Active accounts</span><span className="text-sky-200">● New accounts</span></div></section><section className="panel p-5"><h2 className="font-semibold">Record growth</h2><p className="mt-1 text-xs text-dim">Records observed per day across retained collections</p><div className="mt-5">{rows.length > 1 ? <Chart definition={records} height={220} ariaLabel="Daily observed records" /> : <Empty />}</div></section></div>;
}

const helper = createColumnHelper<CoverageCollection>();

export function CoveragePage() {
  const [sorting, setSorting] = useState<SortingState>([{ id: "records", desc: true }]);
  const [filter, setFilter] = useState("");
  const coverage = useQuery({ queryKey: ["coverage"], queryFn: () => api<CoverageResponse>("/api/coverage"), staleTime: 60_000 });
  const columns = useMemo(() => [
    helper.accessor("namespace", { header: "Namespace", cell: ({ getValue }) => <span className="text-sky-300">{getValue()}</span> }),
    helper.accessor("collection", { header: "Collection", cell: ({ getValue }) => <span className="block max-w-120 break-all font-mono text-xs">{getValue()}</span> }),
    helper.accessor("records", { header: "Records", cell: ({ getValue }) => <span className="font-mono">{getValue().toLocaleString()}</span> }),
    helper.accessor("unique_accounts", { header: "Accounts", cell: ({ getValue }) => <span className="font-mono">{getValue().toLocaleString()}</span> }),
    helper.accessor("compressed_bytes", { header: "Compressed", cell: ({ getValue }) => bytes(getValue()) }),
    helper.accessor("uncompressed_bytes", { header: "Logical", cell: ({ getValue }) => bytes(getValue()) }),
  ], []);
  const table = useReactTable({ data: coverage.data?.collections || [], columns, state: { sorting, globalFilter: filter }, onSortingChange: setSorting, onGlobalFilterChange: setFilter, getCoreRowModel: getCoreRowModel(), getFilteredRowModel: getFilteredRowModel(), getSortedRowModel: getSortedRowModel(), getPaginationRowModel: getPaginationRowModel(), initialState: { pagination: { pageIndex: 0, pageSize: 20 } } });
  if (coverage.isLoading) return <p className="grid min-h-64 place-items-center text-sm text-dim">Calculating retained coverage…</p>;
  if (!coverage.data) return <p className="grid min-h-64 place-items-center text-sm text-red-200">Coverage aggregates are temporarily unavailable.</p>;
  const data = coverage.data, summary = data.summary;
  const repositoryTotal = summary.repository_jobs_queued + summary.repository_jobs_running + summary.repository_jobs_partial + summary.repository_jobs_failed + summary.repository_accounts_completed;
  return <section className="space-y-5"><header><p className="eyebrow">Retained dataset</p><h1 className="mt-2 text-2xl font-semibold">Platform mirror coverage</h1><p className="mt-2 max-w-4xl text-sm text-dim">What this observatory has seen and durably mirrored. Counts describe retained public records and completed repository imports—not private data or guaranteed global Bluesky coverage.</p></header>
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4"><Stat label="Observed actors" value={compact.format(summary.unique_observed_actors)} note="Unique public event actors" /><Stat label="Resolved identities" value={compact.format(summary.identities_resolved)} note="DIDs with a retained handle" /><Stat label="Completed repositories" value={compact.format(summary.repository_accounts_completed)} note={`of ${repositoryTotal.toLocaleString()} tracked repository jobs`} /><Stat label="Unique records" value={compact.format(summary.unique_records)} note={bytes(summary.total_bytes)} /></div>
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5"><Stat label="Queued" value={summary.repository_jobs_queued.toLocaleString()} /><Stat label="Running" value={summary.repository_jobs_running.toLocaleString()} /><Stat label="Partial" value={summary.repository_jobs_partial.toLocaleString()} /><Stat label="Failed" value={summary.repository_jobs_failed.toLocaleString()} /><Stat label="Coverage estimate" value={summary.coverage_estimate == null ? "Unavailable" : `${number.format(summary.coverage_estimate * 100)}%`} note={summary.coverage_denominator == null ? "No trustworthy platform denominator configured" : `${summary.coverage_denominator.toLocaleString()} account denominator`} /></div>
    <div className="rounded-xl border border-sky-500/25 bg-sky-500/5 p-4 text-xs text-sky-100"><b>How to read this:</b> {summary.coverage_qualification}</div>
    <CoverageCharts data={data.daily} />
    <section className="panel overflow-hidden"><div className="flex flex-wrap items-end justify-between gap-3 border-b border-line p-5"><div><h2 className="font-semibold">Collections and namespaces</h2><p className="mt-1 text-xs text-dim">Record and storage distribution by ATProto lexicon</p></div><label className="relative"><Search className="absolute left-3 top-2.5 size-4 text-dim" /><input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter collection…" className="h-9 rounded-lg border border-line bg-black pl-9 pr-3 text-xs outline-none focus:border-sky-500" /></label></div><div className="overflow-x-auto"><table className="w-full min-w-220 text-left text-xs"><thead className="border-b border-line text-dim">{table.getHeaderGroups().map((group) => <tr key={group.id}>{group.headers.map((header) => <th key={header.id} className="px-4 py-3"><button onClick={header.column.getToggleSortingHandler()} className="flex items-center gap-1">{flexRender(header.column.columnDef.header, header.getContext())}<ArrowDownUp className="size-3" /></button></th>)}</tr>)}</thead><tbody>{table.getRowModel().rows.map((row) => <tr key={row.original.collection} className="border-b border-line/60 hover:bg-white/[.025]">{row.getVisibleCells().map((cell) => <td key={cell.id} className="px-4 py-3">{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>)}</tbody></table></div><footer className="flex items-center justify-between p-4 text-xs text-dim"><span>{table.getFilteredRowModel().rows.length.toLocaleString()} collections · page {table.getState().pagination.pageIndex + 1} of {Math.max(1, table.getPageCount())}</span><div className="flex gap-2"><button disabled={!table.getCanPreviousPage()} onClick={() => table.previousPage()} className="rounded border border-line p-2 disabled:opacity-30" aria-label="Previous collection page"><ChevronLeft className="size-4" /></button><button disabled={!table.getCanNextPage()} onClick={() => table.nextPage()} className="rounded border border-line p-2 disabled:opacity-30" aria-label="Next collection page"><ChevronRight className="size-4" /></button></div></footer></section>
  </section>;
}
function Empty() { return <p className="grid h-52 place-items-center text-sm text-dim">Collecting daily coverage history…</p>; }
