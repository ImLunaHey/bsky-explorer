import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  type Cell,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ArrowDownUp, ChevronLeft, ChevronRight, Search } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { api } from "./api";
import { useIdentityMap } from "./identities";
import type {
  BackfillJob,
  BackfillJobsResponse,
  BackfillStatus,
  IdentityQueueResponse,
} from "./types";

const active = new Set<BackfillStatus>([
  "queued",
  "resolving",
  "running",
  "retrying",
]);
const statuses: ("all" | BackfillStatus)[] = [
  "all",
  "queued",
  "resolving",
  "running",
  "retrying",
  "completed",
  "dead_letter",
  "cancelled",
];
const fmt = (value: string) => new Date(value).toLocaleString();
const statusTone = (status: BackfillStatus) =>
  active.has(status)
    ? "bg-orange-400"
    : status === "completed"
      ? "bg-emerald-400"
      : "bg-red-400";

const helper = createColumnHelper<BackfillJob>();
const columns = [
  helper.display({ id: "expand", header: "", cell: () => null }),
  helper.accessor((job) => job.handle || job.did || job.query, {
    id: "account",
    header: "Account",
    cell: (x) => {
      const job = x.row.original;
      return (
        <div className="max-w-64">
          <span className="block break-all font-medium text-orange-300">
            {job.handle
              ? `@${job.handle}`
              : job.did
                ? job.did
                : job.query}
          </span>
          {job.handle && job.did && (
            <span className="block break-all font-mono text-[10px] text-dim">
              {job.did}
            </span>
          )}
          {!job.handle&&job.did&&<span className="block text-[9px] text-dim">resolving…</span>}
        </div>
      );
    },
  }),
  helper.accessor("job_type", {
    header: "Type",
    cell: (x) => <span className="uppercase text-dim">{x.getValue()}</span>,
  }),
  helper.accessor("status", {
    header: "Status",
    cell: (x) => (
      <span className="inline-flex items-center gap-2 uppercase">
        <i className={`size-2 rounded-full ${statusTone(x.getValue())}`} />
        {x.getValue().replace("_", " ")}
      </span>
    ),
  }),
  helper.accessor("records_written", {
    header: "Records",
    cell: (x) => (
      <span className="font-mono">{x.getValue().toLocaleString()}</span>
    ),
  }),
  helper.accessor("retry_count", {
    header: "Attempts",
    cell: (x) => (
      <span className="font-mono">
        {x.getValue() + 1} / {x.row.original.max_retries + 1}
      </span>
    ),
  }),
  helper.accessor("next_attempt_at", {
    header: "Next retry",
    cell: (x) => (x.getValue() ? fmt(x.getValue()!) : "—"),
  }),
  helper.accessor("updated_at", {
    header: "Updated",
    cell: (x) => fmt(x.getValue()),
  }),
];

export function BackfillsPage() {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "updated_at", desc: true },
  ]);
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState<"all" | BackfillStatus>("all");
  const [type, setType] = useState<"all" | "repository" | "associations">(
    "all",
  );
  const jobs = useQuery({
    queryKey: ["backfills", "dashboard"],
    queryFn: () => api<BackfillJobsResponse>("/api/backfills?limit=100"),
    staleTime: 2_000,
    refetchInterval: (q) =>
      q.state.data?.jobs.some((job) => active.has(job.status)) ? 10_000 : false,
  });
  const handles = useIdentityMap(
    (jobs.data?.jobs || []).flatMap((job) => (job.did ? [job.did] : [])),
  );
  const identityQueue = useQuery({
    queryKey: ["identity-queue"],
    queryFn: () => api<IdentityQueueResponse>("/api/identity-queue?limit=25"),
    staleTime: 10_000,
    refetchInterval: (query) => {
      const summary = query.state.data?.summary;
      return summary && (summary.queued || summary.running || summary.retrying)
        ? 10_000
        : false;
    },
  });
  const data = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return (jobs.data?.jobs || []).map((job) => ({...job,handle:job.handle || (job.did ? handles.get(job.did) : undefined) || null})).filter(
      (job) =>
        (status === "all" || job.status === status) &&
        (type === "all" || job.job_type === type) &&
        (!needle ||
          [job.id, job.query, job.did, job.handle, job.last_error].some(
            (value) => value?.toLowerCase().includes(needle),
          )),
    );
  }, [filter, handles, jobs.data, status, type]);
  const table = useReactTable({
    data,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    initialState: { pagination: { pageIndex: 0, pageSize: 20 } },
  });
  const counts = (jobs.data?.jobs || []).reduce(
    (out, job) => {
      out.total++;
      if (active.has(job.status)) out.active++;
      if (job.status === "completed") out.completed++;
      if (job.status === "dead_letter") out.failed++;
      out.records += job.records_written;
      return out;
    },
    { total: 0, active: 0, completed: 0, failed: 0, records: 0 },
  );
  return (
    <section>
      <div>
        <p className="eyebrow">Durable collection queue</p>
        <h1 className="mt-2 text-2xl font-semibold">Backfill jobs</h1>
        <p className="mt-2 text-sm text-dim">
          On-demand public account repositories and association records. Jobs
          persist across restarts.
        </p>
      </div>
      <div className="mt-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
        {(
          [
            ["Total", counts.total],
            ["Active", counts.active],
            ["Completed", counts.completed],
            ["Failed", counts.failed],
            ["Records written", counts.records],
          ] as const
        ).map(([label, value]) => (
          <div key={label} className="panel p-4">
            <b className="text-2xl">{value.toLocaleString()}</b>
            <p className="eyebrow mt-1">{label}</p>
          </div>
        ))}
      </div>
      <section className="panel mt-5 p-5">
        <div className="flex items-start justify-between gap-4">
          <div><h2 className="font-semibold">Identity resolution queue</h2><p className="mt-1 text-xs text-dim">Handles are resolved asynchronously without delaying live pages.</p></div>
          {identityQueue.isFetching&&<span className="animate-pulse text-xs text-orange-300">Updating…</span>}
        </div>
        {identityQueue.data?<div className="mt-4 grid gap-2 sm:grid-cols-3 lg:grid-cols-7">{([['Total',identityQueue.data.summary.total],['Queued',identityQueue.data.summary.queued],['Running',identityQueue.data.summary.running],['Retrying',identityQueue.data.summary.retrying],['Resolved',identityQueue.data.summary.resolved],['Negative',identityQueue.data.summary.negative],['Dead letter',identityQueue.data.summary.dead_letter]] as const).map(([label,value])=><div key={label} className="rounded-lg border border-line p-3"><b className="text-lg">{value.toLocaleString()}</b><p className="mt-1 text-[10px] uppercase tracking-wide text-dim">{label}</p></div>)}</div>:<p className="mt-4 text-sm text-dim">{identityQueue.isError?'Identity queue status is temporarily unavailable.':'Loading identity queue…'}</p>}
      </section>
      <section className="panel mt-5 p-5">
        <div className="flex flex-wrap gap-2">
          <label className="relative min-w-56 flex-1">
            <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-dim" />
            <input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter DID, handle, job, or error…"
              className="h-10 w-full rounded-lg border border-line bg-black pl-9 pr-3 text-sm outline-none focus:border-orange-500/60"
            />
          </label>
          <select
            value={type}
            onChange={(event) => setType(event.target.value as typeof type)}
            className="rounded-lg border border-line bg-black px-3 text-xs"
          >
            <option value="all">All types</option>
            <option value="repository">Repository</option>
            <option value="associations">Associations</option>
          </select>
          <select
            value={status}
            onChange={(event) => setStatus(event.target.value as typeof status)}
            className="rounded-lg border border-line bg-black px-3 text-xs"
          >
            {statuses.map((value) => (
              <option key={value} value={value}>
                {value === "all" ? "All statuses" : value.replace("_", " ")}
              </option>
            ))}
          </select>
        </div>
        {jobs.isLoading ? (
          <div className="flex min-h-64 items-center justify-center text-sm text-dim">
            Loading durable queue…
          </div>
        ) : jobs.isError ? (
          <div className="mt-5 rounded-xl border border-red-500/30 bg-red-500/5 p-5 text-sm text-red-200">
            The durable queue is temporarily unavailable.
          </div>
        ) : (
          <>
            <div className="scrollbar mt-5 overflow-x-auto">
              <table className="w-full min-w-260 text-left text-xs">
                <thead className="border-b border-line text-dim">
                  {table.getHeaderGroups().map((group) => (
                    <tr key={group.id}>
                      {group.headers.map((header) => (
                        <th key={header.id} className="px-3 py-3 font-medium">
                          {header.id === "expand" ? null : (
                            <button
                              className="flex items-center gap-1"
                              onClick={header.column.getToggleSortingHandler()}
                            >
                              {flexRender(
                                header.column.columnDef.header,
                                header.getContext(),
                              )}
                              {header.column.getCanSort() && (
                                <ArrowDownUp className="size-3" />
                              )}
                            </button>
                          )}
                        </th>
                      ))}
                    </tr>
                  ))}
                </thead>
                <tbody>
                  {table.getRowModel().rows.map((row) => (
                    <JobRow key={row.original.id} job={row.original} cells={row.getVisibleCells()} />
                  ))}
                </tbody>
              </table>
              {!data.length && (
                <div className="flex min-h-48 items-center justify-center text-sm text-dim">
                  No jobs match these filters.
                </div>
              )}
            </div>
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs text-dim">
              <span>
                Page {table.getState().pagination.pageIndex + 1} of{" "}
                {Math.max(1, table.getPageCount())} · {data.length} jobs
              </span>
              <div className="flex items-center gap-2">
                <label>
                  Rows{" "}
                  <select
                    value={table.getState().pagination.pageSize}
                    onChange={(e) => table.setPageSize(Number(e.target.value))}
                    className="rounded border border-line bg-black px-2 py-1.5 text-white"
                  >
                    {[10, 20, 50].map((x) => (
                      <option key={x}>{x}</option>
                    ))}
                  </select>
                </label>
                <button
                  disabled={!table.getCanPreviousPage()}
                  onClick={() => table.previousPage()}
                  className="rounded border border-line px-3 py-1.5 text-white disabled:opacity-30"
                >
                  Previous
                </button>
                <button
                  disabled={!table.getCanNextPage()}
                  onClick={() => table.nextPage()}
                  className="rounded border border-line px-3 py-1.5 text-white disabled:opacity-30"
                >
                  Next
                </button>
              </div>
            </div>
          </>
        )}
      </section>
    </section>
  );
}

function JobRow({
  job,
  cells,
}: {
  job: BackfillJob;
  cells: Cell<BackfillJob, unknown>[];
}) {
  return (
      <tr className="border-b border-line/60 hover:bg-white/[.025]">
        {cells.map((cell) => (
          <td key={cell.id} className="px-3 py-3">
            {cell.column.id === "expand" ? (
              <Link to="/backfills/$jobId" params={{jobId:job.id}} aria-label={`Open job ${job.id}`} className="block p-2 text-dim hover:text-orange-300">
                <ChevronRight className="size-4" />
              </Link>
            ) : (
              <Link to="/backfills/$jobId" params={{jobId:job.id}} className="block py-2 text-inherit">
                {flexRender(cell.column.columnDef.cell, cell.getContext())}
              </Link>
            )}
          </td>
        ))}
      </tr>
  );
}

export function BackfillDetailPage({jobId}:{jobId:string}) {
  const job=useQuery({
    queryKey:["backfill",jobId],
    queryFn:()=>api<BackfillJob>(`/api/backfills/${encodeURIComponent(jobId)}/status`),
    staleTime:2_000,
    refetchInterval:query=>query.state.data&&active.has(query.state.data.status)?5_000:false,
  });
  if(job.isLoading)return <div className="panel grid min-h-64 place-items-center text-sm text-dim">Loading job status…</div>;
  if(job.isError||!job.data)return <div className="panel p-6 text-sm text-red-200">This backfill job could not be loaded.</div>;
  const value=job.data;
  return <section>
    <Link to="/backfills" className="inline-flex items-center gap-2 text-sm text-dim hover:text-orange-300"><ChevronLeft className="size-4"/>Backfill jobs</Link>
    <div className="mt-5 flex flex-wrap items-start justify-between gap-4">
      <div><p className="eyebrow">Durable collection job</p><h1 className="mt-2 break-all text-2xl font-semibold">{value.handle?`@${value.handle}`:value.did||value.query}</h1><p className="mt-2 break-all font-mono text-xs text-dim">{value.id}</p></div>
      <span className="inline-flex items-center gap-2 rounded-full border border-line px-3 py-2 text-xs uppercase"><i className={`size-2 rounded-full ${statusTone(value.status)}`}/>{value.status.replace("_"," ")}</span>
    </div>
    <div className="panel mt-6 p-6">
      <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-4">
        <Detail label="Type" value={value.job_type}/><Detail label="Created" value={fmt(value.created_at)}/><Detail label="Updated" value={fmt(value.updated_at)}/><Detail label="Collection" value={value.collection||"All public records"}/>
        <Detail label="Progress" value={`${value.records_written.toLocaleString()} records written`}/><Detail label="Cursor" value={value.cursor||"—"}/><Detail label="Attempts" value={`${value.retry_count+1} / ${value.max_retries+1}`}/><Detail label="Next attempt" value={value.next_attempt_at?fmt(value.next_attempt_at):"—"}/>
        <Detail label="Dependency" value={value.depends_on_job_id||"None"}/><Detail label="Error" value={value.last_error||"None"}/>
      </div>
      <div className="mt-6 flex flex-wrap gap-3">
        {value.did&&<Link to="/accounts/$did" params={{did:value.did}} className="rounded-lg border border-orange-500/40 px-4 py-2 text-sm text-orange-300 hover:bg-orange-500/10">Open account</Link>}
        {value.depends_on_job_id&&<Link to="/backfills/$jobId" params={{jobId:value.depends_on_job_id}} className="rounded-lg border border-line px-4 py-2 text-sm text-white hover:border-orange-500/40">Open dependency</Link>}
      </div>
    </div>
  </section>;
}
function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <p className="eyebrow">{label}</p>
      <p className="mt-1 break-all text-xs text-white/80">{value}</p>
    </div>
  );
}
