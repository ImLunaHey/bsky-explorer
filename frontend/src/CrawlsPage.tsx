import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  type ColumnDef,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ArrowDownUp, ChevronLeft, Pause, Play, Square } from "lucide-react";
import { api } from "./api";
import type {
  BackfillJob,
  CrawlAction,
  CrawlDetailResponse,
  CreateCrawlResponse,
  CrawlLimitKey,
  CrawlRelation,
  CrawlsResponse,
} from "./types";

const relations: { value: CrawlRelation; label: string; hint: string }[] = [
  { value: "posts", label: "Posts", hint: "Repository records authored by each account" },
  { value: "likes", label: "Likes", hint: "Accounts liking retained posts" },
  { value: "reposts", label: "Reposts", hint: "Accounts amplifying retained posts" },
  { value: "quotes", label: "Quotes", hint: "Quote-post relationships" },
  { value: "replies", label: "Replies", hint: "Replies and conversation participants" },
  { value: "profiles", label: "Profiles", hint: "Resolve discovered identities without crawling their repositories" },
];
const activeCrawl = (status?: string) => status === "queued" || status === "running";
const activeJob = (status?: string) => ["queued", "resolving", "running", "retrying"].includes(status || "");
const accountDepth = (job: BackfillJob) => job.crawl_depth || 0;
const fmt = (value?: string | null) => value ? new Date(value).toLocaleString() : "—";
const duration = (seconds?: number | null) => seconds == null ? "—" : seconds < 60 ? `${Math.ceil(seconds)} sec` : seconds < 3600 ? `${Math.ceil(seconds / 60)} min` : seconds < 86400 ? `${(seconds / 3600).toFixed(1)} hr` : `${(seconds / 86400).toFixed(1)} days`;

async function post<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    headers: body == null ? undefined : { "content-type": "application/json" },
    body: body == null ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error((await response.text()) || `${response.status} ${response.statusText}`);
  return response.json() as Promise<T>;
}

export function CrawlOverview() {
  const queryClient = useQueryClient();
  const [root, setRoot] = useState("");
  const [maxDepth, setMaxDepth] = useState(1);
  const [selected, setSelected] = useState<CrawlRelation[]>(["posts", "likes", "reposts", "quotes", "replies", "profiles"]);
  const [limits, setLimits] = useState<Record<CrawlLimitKey, string>>({ max_accounts: "250", max_jobs: "500", max_records: "1000000", max_requests: "250000" });
  const crawls = useQuery({
    queryKey: ["crawls"],
    queryFn: () => api<CrawlsResponse>("/api/crawls?limit=20&offset=0"),
    refetchInterval: (query) => query.state.data?.crawls.some((crawl) => activeCrawl(crawl.status)) ? 10_000 : false,
  });
  const create = useMutation({
    mutationFn: () => post<CreateCrawlResponse>("/api/crawls", {
      root: root.trim(),
      max_depth: maxDepth,
      relations: selected,
      limits: Object.fromEntries(Object.entries(limits).map(([key, value]) => [key, Number(value)])),
    }),
    onSuccess: () => { setRoot(""); void queryClient.invalidateQueries({ queryKey: ["crawls"] }); void queryClient.invalidateQueries({ queryKey: ["backfills"] }); },
  });
  const toggle = (relation: CrawlRelation) => setSelected((current) => current.includes(relation) ? current.filter((item) => item !== relation) : [...current, relation]);
  return (
    <>
      <section className="panel mt-5 p-5">
        <div className="flex flex-wrap items-start justify-between gap-3"><div><h2 className="font-semibold">Start bounded crawl</h2><p className="mt-1 text-xs text-dim">Backfill one account and explicitly control how far association discovery may expand.</p></div><span className="rounded-full border border-sky-500/30 px-3 py-1 text-[10px] uppercase tracking-wide text-sky-300">Maximum depth 3</span></div>
        <form className="mt-5 space-y-5" onSubmit={(event) => { event.preventDefault(); if (root.trim() && selected.length) create.mutate(); }}>
          <div className="grid gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(10rem,1fr)]"><label className="text-xs text-dim">Handle or DID<input required value={root} onChange={(event) => setRoot(event.target.value)} placeholder="example.com or did:plc:…" className="mt-2 h-10 w-full rounded-lg border border-line bg-black px-3 text-sm text-white outline-none focus:border-sky-500" /></label><label className="text-xs text-dim">Maximum account depth<select value={maxDepth} onChange={(event) => setMaxDepth(Number(event.target.value))} className="mt-2 h-10 w-full rounded-lg border border-line bg-black px-3 text-sm text-white"><option value={0}>0 · selected account only</option><option value={1}>1 · direct participants</option><option value={2}>2 · participants of participants</option><option value={3}>3 · maximum expansion</option></select></label></div>
          <fieldset><legend className="text-xs text-dim">Relationships and records</legend><div className="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">{relations.map((relation) => <label key={relation.value} className={`cursor-pointer rounded-lg border p-3 ${selected.includes(relation.value) ? "border-sky-500/40 bg-sky-500/5" : "border-line"}`}><span className="flex items-center gap-2"><input type="checkbox" checked={selected.includes(relation.value)} onChange={() => toggle(relation.value)} className="accent-sky-500" /><b className="text-sm">{relation.label}</b></span><span className="mt-1 block pl-5 text-[10px] text-dim">{relation.hint}</span></label>)}</div></fieldset>
          <fieldset><legend className="text-xs text-dim">Hard safety limits</legend><div className="mt-2 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">{(Object.keys(limits) as CrawlLimitKey[]).map((key) => <label key={key} className="text-[10px] uppercase tracking-wide text-dim">{key.replace("max_", "Max ").replace("_", " ")}<input type="number" required min={1} value={limits[key]} onChange={(event) => setLimits((current) => ({ ...current, [key]: event.target.value }))} className="mt-1 h-9 w-full rounded-lg border border-line bg-black px-3 font-mono text-xs text-white" /></label>)}</div></fieldset>
          {create.error && <p className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-xs text-red-200">{create.error.message}</p>}
          <div className="flex flex-wrap items-center justify-between gap-3"><p className="max-w-2xl text-xs text-dim">Depth 0 is the selected account. Higher depths may enqueue repository work for discovered accounts, but the hard limits stop unbounded expansion.</p><button disabled={create.isPending || !root.trim() || !selected.length} className="rounded-lg bg-sky-500 px-5 py-2 text-sm font-semibold text-black disabled:opacity-40">{create.isPending ? "Creating…" : "Create crawl"}</button></div>
        </form>
      </section>
      <section className="panel mt-5 overflow-hidden"><div className="border-b border-line p-5"><h2 className="font-semibold">Crawls</h2><p className="mt-1 text-xs text-dim">Each crawl groups its repository, association, and identity work into a bounded tree.</p></div>{crawls.isLoading ? <p className="grid min-h-32 place-items-center text-sm text-dim">Loading crawls…</p> : crawls.isError ? <p className="p-5 text-sm text-red-200">Crawl status is temporarily unavailable.</p> : crawls.data?.crawls.length ? <div className="divide-y divide-line/60">{crawls.data.crawls.map((crawl) => <Link key={crawl.id} to="/crawls/$crawlId" params={{ crawlId: crawl.id }} className="grid gap-3 p-4 hover:bg-white/[.025] sm:grid-cols-[minmax(0,2fr)_repeat(3,minmax(0,1fr))]"><div><b className="break-all text-sm text-sky-300">{crawl.root_did || crawl.root_query}</b><p className="mt-1 font-mono text-[9px] text-dim">{crawl.id}</p></div><Mini label="Status" value={crawl.status} /><Mini label="Depth" value={`0–${crawl.max_depth}`} /><Mini label="Updated" value={fmt(crawl.updated_at)} /></Link>)}</div> : <p className="grid min-h-32 place-items-center text-sm text-dim">No crawls have been created yet.</p>}</section>
    </>
  );
}

export function CrawlDetailPage({ crawlId }: { crawlId: string }) {
  const queryClient = useQueryClient();
  const [sorting, setSorting] = useState<SortingState>([{ id: "updated_at", desc: true }]);
  const detail = useQuery({ queryKey: ["crawl", crawlId], queryFn: () => api<CrawlDetailResponse>(`/api/crawls/${encodeURIComponent(crawlId)}`), refetchInterval: (query) => activeCrawl(query.state.data?.crawl.status) ? 5_000 : false });
  const refresh = () => { void queryClient.invalidateQueries({ queryKey: ["crawl", crawlId] }); void queryClient.invalidateQueries({ queryKey: ["crawls"] }); void queryClient.invalidateQueries({ queryKey: ["backfills"] }); };
  const action = useMutation({ mutationFn: ({ path }: { path: string }) => post<unknown>(path), onSuccess: refresh });
  const runAction = (name: CrawlAction) => action.mutate({ path: `/api/crawls/${encodeURIComponent(crawlId)}/${name}` });
  const columns = useMemo<ColumnDef<BackfillJob>[]>(() => [
    { accessorKey: "job_type", header: "Phase", cell: ({ row }) => <div><b className="uppercase text-sky-300">{row.original.progress?.kind || row.original.job_type}</b><p className="mt-1 text-[9px] text-dim">{row.original.progress?.current_collection || row.original.collection || "Waiting"}</p></div> },
    { accessorKey: "did", header: "Account", cell: ({ row }) => <div className="max-w-64"><b className="block break-all">{row.original.handle ? `@${row.original.handle}` : row.original.did || row.original.query}</b>{row.original.handle && row.original.did && <span className="block break-all font-mono text-[9px] text-dim">{row.original.did}</span>}</div> },
    { accessorKey: "status", header: "Status", cell: ({ row }) => <span className="uppercase">{row.original.status.replace("_", " ")}</span> },
    { accessorKey: "records_written", header: "Records", cell: ({ row }) => (row.original.progress?.records_written ?? row.original.records_written).toLocaleString() },
    { id: "requests", accessorFn: (job) => job.progress?.requests, header: "Requests", cell: ({ row }) => row.original.progress?.requests?.toLocaleString() || "—" },
    { id: "rate", accessorFn: (job) => job.progress?.rate_per_second, header: "Rate", cell: ({ row }) => row.original.progress?.rate_per_second == null ? "—" : `${row.original.progress.rate_per_second.toFixed(2)}/s` },
    { id: "eta", accessorFn: (job) => job.progress?.eta_seconds, header: "ETA", cell: ({ row }) => duration(row.original.progress?.eta_seconds) },
    { accessorKey: "lease_until", header: "Worker lease", cell: ({ row }) => row.original.status === "running" && row.original.lease_until ? <span className="block max-w-40 text-amber-300" title={fmt(row.original.lease_until)}>Active until {new Date(row.original.lease_until).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span> : "—" },
    { accessorKey: "updated_at", header: "Updated", cell: ({ row }) => fmt(row.original.updated_at) },
    { id: "controls", header: "Controls", enableSorting: false, cell: ({ row }) => <JobControls job={row.original} pending={action.isPending} onAction={(name, scope) => action.mutate({ path: `/api/backfills/${encodeURIComponent(row.original.id)}/${name}?scope=${scope}` })} /> },
  ], [action]);
  const jobs = useMemo(() => detail.data?.jobs || [], [detail.data?.jobs]);
  const byDepth = useMemo(() => Array.from(new Set(jobs.map(accountDepth))).sort((a, b) => a - b).map((depth) => ({ depth, jobs: jobs.filter((job) => accountDepth(job) === depth) })), [jobs]);
  if (detail.isLoading) return <p className="grid min-h-64 place-items-center text-sm text-dim">Loading crawl…</p>;
  if (!detail.data) return <p className="grid min-h-64 place-items-center text-sm text-red-200">This crawl could not be loaded.</p>;
  const crawl = detail.data.crawl;
  const requestsUsed = jobs.reduce((sum, job) => sum + (job.progress?.requests || 0), 0);
  return <section><Link to="/backfills" className="inline-flex items-center gap-2 text-sm text-dim hover:text-sky-300"><ChevronLeft className="size-4" />Backfills</Link><header className="mt-5 flex flex-wrap items-start justify-between gap-4"><div><p className="eyebrow">Bounded crawl</p><h1 className="mt-2 break-all text-2xl font-semibold">{crawl.root_did || crawl.root_query}</h1><p className="mt-1 break-all font-mono text-xs text-dim">{crawl.id}</p></div><span className="rounded-full border border-sky-500/30 px-3 py-2 text-xs uppercase text-sky-300">{crawl.status}</span></header>
    <div className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-4"><Card label="Account depth" value={`0–${crawl.max_depth}`} /><Card label="Jobs" value={jobs.length.toLocaleString()} /><Card label="Relations" value={crawl.relations.join(", ")} /><Card label="Updated" value={fmt(crawl.updated_at)} /></div>
    <section className="panel mt-5 p-5"><div className="flex flex-wrap items-start justify-between gap-4"><div><h2 className="font-semibold">Crawl controls</h2><p className="mt-1 text-xs text-dim">Pause stops future claims; leased work transitions safely when its lease ends.</p></div><div className="flex gap-2">{crawl.status !== "paused" && crawl.status !== "cancelled" && <button disabled={action.isPending} onClick={() => runAction("pause")} className="inline-flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs"><Pause className="size-3" />Pause crawl</button>}{crawl.status === "paused" && <button disabled={action.isPending} onClick={() => runAction("resume")} className="inline-flex items-center gap-2 rounded-lg border border-sky-500/40 px-3 py-2 text-xs text-sky-300"><Play className="size-3" />Resume crawl</button>}{crawl.status !== "cancelled" && <button disabled={action.isPending} onClick={() => runAction("cancel")} className="inline-flex items-center gap-2 rounded-lg border border-red-500/30 px-3 py-2 text-xs text-red-300"><Square className="size-3" />Cancel crawl</button>}</div></div><div className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-5"><Mini label="Request budget used" value={`${requestsUsed.toLocaleString()} / ${crawl.limits.max_requests?.toLocaleString() || "unlimited"}`} />{(Object.entries(crawl.limits || {}) as [string, number][]).map(([key, value]) => <Mini key={key} label={key.replace("max_", "Max ").replace("_", " ")} value={value.toLocaleString()} />)}</div></section>
    {action.error && <div role="alert" className="mt-5 rounded-xl border border-amber-500/40 bg-amber-500/10 p-4 text-sm text-amber-100"><b className="block">The requested queue control was not applied</b><p className="mt-1">{action.error.message.includes("leased running jobs") ? "A worker currently owns an active lease for this job. Wait until the lease time shown below expires, then try again; the running process is not interrupted unsafely." : action.error.message}</p></div>}
    <div className="mt-5 space-y-5">{byDepth.map((group) => <DepthTable key={group.depth} depth={group.depth} jobs={group.jobs} columns={columns} sorting={sorting} setSorting={setSorting} />)}</div>
  </section>;
}

function DepthTable({ depth, jobs, columns, sorting, setSorting }: { depth: number; jobs: BackfillJob[]; columns: ColumnDef<BackfillJob>[]; sorting: SortingState; setSorting: React.Dispatch<React.SetStateAction<SortingState>> }) {
  // The table is independently bounded to one crawl depth, preserving lineage without a huge expanded row.
  const table = useReactTable({ data: jobs, columns, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel() });
  return <section className="panel overflow-hidden"><div className="flex items-center justify-between border-b border-line p-4"><div><h2 className="font-semibold">Account depth {depth}</h2><p className="mt-1 text-xs text-dim">{depth === 0 ? "Selected root account and its helper phases" : `Accounts discovered at relationship depth ${depth}; association helper jobs stay with their owning account`}</p></div><span className="font-mono text-xs text-dim">{jobs.length.toLocaleString()} jobs</span></div><div className="overflow-x-auto"><table className="w-full min-w-280 text-left text-xs"><thead className="border-b border-line text-dim">{table.getHeaderGroups().map((group) => <tr key={group.id}>{group.headers.map((header) => <th key={header.id} className="px-3 py-3"><button disabled={!header.column.getCanSort()} onClick={header.column.getToggleSortingHandler()} className="flex items-center gap-1 disabled:cursor-default">{flexRender(header.column.columnDef.header, header.getContext())}{header.column.getCanSort() && <ArrowDownUp className="size-3" />}</button></th>)}</tr>)}</thead><tbody>{table.getRowModel().rows.map((row) => <tr key={row.original.id} className="border-b border-line/60 hover:bg-white/[.025]">{row.getVisibleCells().map((cell) => <td key={cell.id} className="px-3 py-3 align-top">{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>)}</tbody></table></div></section>;
}
function Card({ label, value }: { label: string; value: string }) { return <div className="panel p-4"><p className="eyebrow">{label}</p><p className="mt-2 break-words text-lg font-semibold">{value}</p></div>; }
function Mini({ label, value }: { label: string; value: string }) { return <div className="min-w-0"><p className="text-[9px] uppercase tracking-wide text-dim">{label}</p><p className="mt-1 break-words text-xs text-white/80">{value}</p></div>; }
function JobControls({ job, pending, onAction }: { job: BackfillJob; pending: boolean; onAction: (action: CrawlAction, scope: "job" | "branch") => void }) {
  const [scope, setScope] = useState<"job" | "branch">("job");
  return <div className="flex min-w-max items-center gap-1"><Link to="/backfills/$jobId" params={{ jobId: job.id }} className="rounded border border-line px-2 py-1 text-sky-300">Details</Link><select value={scope} onChange={(event) => setScope(event.target.value as "job" | "branch")} aria-label={`Control scope for ${job.id}`} className="rounded border border-line bg-black px-2 py-1"><option value="job">This job</option><option value="branch">This branch</option></select>{activeJob(job.status) && <button disabled={pending} onClick={() => onAction("pause", scope)} className="rounded border border-line px-2 py-1 disabled:opacity-40">Pause</button>}{job.status === "paused" && <button disabled={pending} onClick={() => onAction("resume", scope)} className="rounded border border-sky-500/30 px-2 py-1 text-sky-300 disabled:opacity-40">Resume</button>}{!(["completed", "cancelled"] as string[]).includes(job.status) && <button disabled={pending} onClick={() => onAction("cancel", scope)} className="rounded border border-red-500/30 px-2 py-1 text-red-300 disabled:opacity-40">Cancel</button>}</div>;
}
