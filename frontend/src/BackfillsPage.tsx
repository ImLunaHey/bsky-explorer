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
import { IdentityLink, IdentityName, useIdentityMap } from "./identities";
import { CrawlOverview } from "./CrawlsPage";
import type {
  BackfillJob,
  BackfillEvent,
  BackfillEventsResponse,
  BackfillJobsResponse,
  BackfillStatus,
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
  "paused",
  "completed",
  "dead_letter",
  "cancelled",
];
const fmt = (value: string) => new Date(value).toLocaleString();
const statusTone = (status: BackfillStatus) =>
  active.has(status)
    ? "bg-sky-400"
    : status === "paused"
      ? "bg-amber-400"
    : status === "completed"
      ? "bg-emerald-400"
      : "bg-red-400";

const helper = createColumnHelper<BackfillJob>();
const eventHelper=createColumnHelper<BackfillEvent>();
const columns = [
  helper.display({ id: "expand", header: "", cell: () => null }),
  helper.accessor((job) => job.handle || job.did || job.query, {
    id: "account",
    header: "Account",
    cell: (x) => {
      const job = x.row.original;
      return (
        <div className="max-w-64">
          {job.did ? <IdentityName did={job.did} handle={job.handle || undefined} /> : job.query}
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
      <CrawlOverview />
      <section className="panel mt-5 p-5">
        <div className="flex flex-wrap gap-2">
          <label className="relative min-w-56 flex-1">
            <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-dim" />
            <input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter DID, handle, job, or error…"
              className="h-10 w-full rounded-lg border border-line bg-black pl-9 pr-3 text-sm outline-none focus:border-sky-500/60"
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
              <Link to="/backfills/$jobId" params={{jobId:job.id}} aria-label={`Open job ${job.id}`} className="block p-2 text-dim hover:text-sky-300">
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
  const [eventOffset,setEventOffset]=useState(0);
  const [eventSearch,setEventSearch]=useState("");
  const [eventSorting,setEventSorting]=useState<SortingState>([{id:"timestamp_ms",desc:true}]);
  const job=useQuery({
    queryKey:["backfill",jobId],
    queryFn:()=>api<BackfillJob>(`/api/backfills/${encodeURIComponent(jobId)}/status`),
    staleTime:2_000,
    refetchInterval:query=>query.state.data&&active.has(query.state.data.status)?5_000:false,
  });
  const did=job.data?.did;
  const resolved=useIdentityMap(did?[did]:[]).get(did||"");
  const imported=useQuery({
    queryKey:["backfill-events",jobId,eventOffset],
    queryFn:()=>api<BackfillEventsResponse>(`/api/backfills/${encodeURIComponent(jobId)}/events?limit=50&offset=${eventOffset}`),
    enabled:Boolean(did),staleTime:5_000,
    refetchInterval:()=>job.data&&active.has(job.data.status)?10_000:false,
  });
  const eventColumns=useMemo(()=>[
    eventHelper.accessor("timestamp_ms",{header:"Observed",cell:x=><time className="whitespace-nowrap text-dim">{new Date(x.getValue()).toLocaleString()}</time>}),
    eventHelper.accessor("kind",{header:"Kind",cell:x=><span className="uppercase text-sky-300">{x.getValue()}</span>}),
    eventHelper.accessor("actor_did",{header:"Actor",cell:x=><IdentityLink did={x.getValue()} compact className="max-w-56" />}),
    eventHelper.accessor("collection",{header:"Collection",cell:x=><span className="block max-w-64 truncate" title={x.getValue()}>{x.getValue()}</span>}),
    eventHelper.accessor(x=>x.at_uri||x.target,{id:"record",header:"Record / target",cell:x=>{const uri=x.getValue();return uri?<Link to="/records/$uri" params={{uri}} className="block max-w-80 truncate text-sky-300" title={uri}>{uri}</Link>:"—"}}),
  ],[]);
  const eventData=useMemo(()=>{const needle=eventSearch.trim().toLowerCase();return (imported.data?.events||[]).filter(event=>!needle||`${event.actor_did} ${event.kind} ${event.collection} ${event.at_uri} ${event.target}`.toLowerCase().includes(needle))},[eventSearch,imported.data]);
  const eventTable=useReactTable({data:eventData,columns:eventColumns,state:{sorting:eventSorting},onSortingChange:setEventSorting,getCoreRowModel:getCoreRowModel(),getSortedRowModel:getSortedRowModel()});
  if(job.isLoading)return <div className="panel grid min-h-64 place-items-center text-sm text-dim">Loading job status…</div>;
  if(job.isError||!job.data)return <div className="panel p-6 text-sm text-red-200">This backfill job could not be loaded.</div>;
  const value=job.data;
  const handle=value.handle||resolved;
  const progress=value.progress;
  return <section>
    <Link to="/backfills" className="inline-flex items-center gap-2 text-sm text-dim hover:text-sky-300"><ChevronLeft className="size-4"/>Backfill jobs</Link>
    <div className="mt-5 flex flex-wrap items-start justify-between gap-4">
      <div><p className="eyebrow">Durable collection job</p><h1 className="mt-2 break-all text-2xl font-semibold">{handle?`@${handle}`:value.did||value.query}</h1>{value.did&&<p className="mt-1 break-all font-mono text-xs text-dim">{value.did}</p>}<p className="mt-2 break-all font-mono text-xs text-dim">{value.id}</p></div>
      <span className="inline-flex items-center gap-2 rounded-full border border-line px-3 py-2 text-xs uppercase"><i className={`size-2 rounded-full ${statusTone(value.status)}`}/>{value.status.replace("_"," ")}</span>
    </div>
    <div className="panel mt-6 p-6">
      <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-4">
        <Detail label="Type" value={value.job_type}/><Detail label="Created" value={fmt(value.created_at)}/><Detail label="Updated" value={fmt(value.updated_at)}/><Detail label="Collection" value={value.collection||"All public records"}/>
        <Detail label="Progress" value={`${(progress?.records_written??value.records_written).toLocaleString()} records written`}/><Detail label="Cursor" value={progress?.cursor||value.cursor||"—"}/><Detail label="Attempts" value={`${value.retry_count+1} / ${value.max_retries+1}`}/><Detail label="Next attempt" value={value.next_attempt_at?fmt(value.next_attempt_at):"—"}/>
        <Detail label="Dependency" value={value.depends_on_job_id||"None"}/><Detail label="Scope / depth" value={`${value.scope||"—"} · ${value.crawl_depth??0}/${value.max_depth??1}`}/><Detail label="Lease until" value={value.lease_until?fmt(value.lease_until):"—"}/><Detail label="Error" value={value.last_error||"None"}/>
      </div>
      {progress&&<><h2 className="eyebrow mb-4 mt-8">Live checkpoint · {fmt(progress.updated_at)}</h2><div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-4"><Detail label="Current collection" value={progress.current_collection||"—"}/><Detail label="Collection index" value={progress.collection_index?.toLocaleString()||"—"}/><Detail label="Posts processed" value={progress.posts_total!=null?`${(progress.posts_processed||0).toLocaleString()} / ${progress.posts_total.toLocaleString()}`:(progress.posts_processed?.toLocaleString()||"—")}/><Detail label="Remaining" value={progress.posts_remaining?.toLocaleString()||"—"}/><Detail label="Successful posts" value={progress.posts_completed?.toLocaleString()||"—"}/><Detail label="Requests" value={progress.requests?.toLocaleString()||"—"}/><Detail label="Likes / reposts" value={`${(progress.likes||0).toLocaleString()} / ${(progress.reposts||0).toLocaleString()}`}/><Detail label="Quotes / replies" value={`${(progress.quotes||0).toLocaleString()} / ${(progress.replies||0).toLocaleString()}`}/><Detail label="Errors" value={progress.errors?.toLocaleString()||"0"}/><Detail label="Rate" value={progress.rate_per_second!=null?`${progress.rate_per_second.toFixed(2)} / sec`:"—"}/><Detail label="ETA" value={progress.eta_seconds!=null?friendlyDuration(progress.eta_seconds):"—"}/></div></>}
      <div className="mt-6 flex flex-wrap gap-3">
        {value.did&&<Link to="/accounts/$did" params={{did:value.did}} className="rounded-lg border border-sky-500/40 px-4 py-2 text-sm text-sky-300 hover:bg-sky-500/10">Open account</Link>}
        {value.depends_on_job_id&&<Link to="/backfills/$jobId" params={{jobId:value.depends_on_job_id}} className="rounded-lg border border-line px-4 py-2 text-sm text-white hover:border-sky-500/40">Open dependency</Link>}
      </div>
    </div>
    <section className="panel mt-6 overflow-hidden"><div className="flex flex-wrap items-end justify-between gap-3 border-b border-line p-5"><div><h2 className="font-semibold">Imported records and interactions</h2><p className="mt-1 text-xs text-dim">Strictly matched to this job's DID and import source.</p></div><input value={eventSearch} onChange={event=>setEventSearch(event.target.value)} placeholder="Filter this page…" className="rounded-lg border border-line bg-black px-3 py-2 text-sm"/></div>
      {imported.isLoading?<div className="grid min-h-40 place-items-center text-sm text-dim">Loading imported events…</div>:imported.isError?<div className="p-5 text-sm text-red-300">Imported events are temporarily unavailable.</div>:<><div className="scrollbar overflow-x-auto"><table className="w-full min-w-250 text-left text-xs"><thead className="border-b border-line text-dim">{eventTable.getHeaderGroups().map(group=><tr key={group.id}>{group.headers.map(header=><th key={header.id} className="px-4 py-3"><button className="flex items-center gap-1" onClick={header.column.getToggleSortingHandler()}>{flexRender(header.column.columnDef.header,header.getContext())}<ArrowDownUp className="size-3"/></button></th>)}</tr>)}</thead><tbody>{eventTable.getRowModel().rows.map(row=><tr key={row.original.event_id} className="border-b border-line/60">{row.getVisibleCells().map(cell=><td key={cell.id} className="px-4 py-3">{flexRender(cell.column.columnDef.cell,cell.getContext())}</td>)}</tr>)}</tbody></table></div><div className="flex items-center justify-between p-4 text-xs text-dim"><span>{(imported.data?.total||0).toLocaleString()} matched · {eventOffset+1}–{Math.min(eventOffset+50,imported.data?.total||0)}</span><div className="flex gap-2"><button disabled={!eventOffset} onClick={()=>setEventOffset(Math.max(0,eventOffset-50))} className="rounded border border-line px-3 py-1.5 disabled:opacity-30">Previous</button><button disabled={eventOffset+50>=(imported.data?.total||0)} onClick={()=>setEventOffset(eventOffset+50)} className="rounded border border-line px-3 py-1.5 disabled:opacity-30">Next</button></div></div></>}
    </section>
  </section>;
}
function friendlyDuration(seconds:number){if(seconds<60)return `${Math.ceil(seconds)} sec`;if(seconds<3600)return `${Math.ceil(seconds/60)} min`;if(seconds<86400)return `${(seconds/3600).toFixed(1)} hr`;return `${(seconds/86400).toFixed(1)} days`}
function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <p className="eyebrow">{label}</p>
      <p className="mt-1 break-all text-xs text-white/80">{value}</p>
    </div>
  );
}
