import { useCallback, useEffect, useState } from "react";
import {
  keepPreviousData,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import {
  ArrowLeft,
  ArrowRight,
  Clock3,
  DatabaseZap,
  Search,
  UserRound,
} from "lucide-react";
import { api } from "./api";
import { useIdentityMap } from "./identities";
import type {
  BackfillJob,
  BackfillJobsResponse,
  SearchKind,
  SearchResponse,
  SearchResult,
} from "./types";

const PAGE_SIZE = 25;
const kinds: { value: SearchKind; label: string }[] = [
  { value: "all", label: "Everything" },
  { value: "account", label: "Accounts" },
  { value: "record", label: "Posts" },
  { value: "event", label: "Events" },
];
function Result({ item }: { item: SearchResult }) {
  const identities = useIdentityMap(item.handle ? [] : [item.did]);
  const handle = item.handle || identities.get(item.did);
  const account = (
    <Link
      to="/accounts/$did"
      params={{ did: item.did }}
      title={`${handle ? `@${handle} · ` : ""}${item.did}`}
      className="block min-w-0 text-sky-300 hover:text-sky-200"
    >
      <span className="block break-all font-medium">
        {handle ? `@${handle}` : item.did}
      </span>
      {handle ? (
        <span className="block break-all font-mono text-[10px] text-dim">
          {item.did}
        </span>
      ) : <span className="block text-[9px] text-dim">resolving…</span>}
    </Link>
  );
  if (item.result_type === "account")
    return (
      <article className="rounded-xl border border-line bg-panel p-5">
        <div className="flex items-start gap-3">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl border border-sky-500/20 bg-sky-500/10">
            <UserRound className="size-5 text-sky-400" />
          </div>
          <div className="min-w-0">
            <h2 className="font-medium">{account}</h2>
            <p className="mt-1 break-all font-mono text-[11px] text-dim">
              {item.did}
            </p>
          </div>
        </div>
      </article>
    );
  const canOpen = Boolean(item.at_uri?.startsWith("at://"));
  return (
    <article className="rounded-xl border border-line bg-panel p-5 transition hover:border-sky-500/30">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
        <span className="rounded-md bg-sky-500/10 px-2 py-1 uppercase tracking-wide text-sky-300">
          {item.event_type || item.operation || item.result_type}
        </span>
        {item.collection && (
          <span className="font-mono text-[10px] text-dim">
            {item.collection}
          </span>
        )}
        {account}
        {item.timestamp_ms != null && (
          <time className="text-dim">
            {new Date(item.timestamp_ms).toLocaleString()}
          </time>
        )}
      </div>
      {item.text ? (
        <p className="mt-4 whitespace-pre-wrap break-words text-sm leading-6 text-white/90">
          {item.text}
        </p>
      ) : (
        item.target && (
          <p className="mt-4 break-all font-mono text-xs text-dim">
            Target: {item.target}
          </p>
        )
      )}
      {item.record_json && (
        <details className="mt-4">
          <summary className="cursor-pointer text-xs text-sky-300">
            Show complete record data
          </summary>
          <pre className="scrollbar mt-3 max-h-80 overflow-auto rounded-lg bg-black p-4 text-[11px] leading-5 text-white/70">
            {item.record_json}
          </pre>
        </details>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        {canOpen && (
          <Link
            to="/records/$uri"
            params={{ uri: item.at_uri! }}
            className="text-xs font-medium text-sky-300 hover:text-sky-200"
          >
            View full record →
          </Link>
        )}
        {item.urls.slice(0, 3).map((url) => (
          <span
            key={url}
            title={url}
            className="max-w-80 truncate rounded bg-white/5 px-2 py-1 text-[10px] text-dim"
          >
            {url}
          </span>
        ))}
      </div>
    </article>
  );
}

const activeStatuses = new Set(["queued", "resolving", "running", "retrying"]);
function jobTime(value?: string | null) {
  if (!value) return null;
  const time = new Date(value);
  return Number.isNaN(time.valueOf()) ? null : time;
}

function BackfillState({
  initial,
  onCompleted,
  compact = false,
}: {
  initial: BackfillJob;
  onCompleted?: () => void;
  compact?: boolean;
}) {
  const status = useQuery({
    queryKey: ["backfill", initial.id],
    queryFn: () =>
      api<BackfillJob>(
        `/api/backfills/${encodeURIComponent(initial.id)}/status`,
      ),
    initialData: initial,
    enabled: !compact,
    refetchInterval: (query) =>
      activeStatuses.has(query.state.data?.status || "") ? 2_000 : false,
    staleTime: 1_000,
  });
  const job = compact ? initial : status.data || initial;
  useEffect(() => {
    if (!compact && job.status === "completed") onCompleted?.();
  }, [compact, job.status, onCompleted]);
  const retryAt = jobTime(job.next_attempt_at);
  const waiting = retryAt && retryAt.valueOf() > Date.now();
  const label = job.status.replace("_", " ");
  if (compact)
    return (
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={`size-2 shrink-0 rounded-full ${activeStatuses.has(job.status) ? "animate-pulse bg-sky-400" : job.status === "completed" ? "bg-emerald-400" : "bg-red-400"}`}
        />
        <span className="min-w-0 flex-1 break-all">
          {job.handle ? <><span className="block">@{job.handle}</span>{job.did&&<span className="block font-mono text-[9px] text-dim">{job.did}</span>}</> : job.did || job.query}
        </span>
        <span className="shrink-0 text-[10px] uppercase text-dim">
          {job.job_type}
        </span>
        <span className="shrink-0 uppercase text-dim">{label}</span>
        <span className="shrink-0 font-mono text-sky-300">
          {job.records_written} records
        </span>
      </div>
    );
  const repository = job.job_type === "repository";
  const waitingForDependency = job.depends_on_job_id && job.status === "queued";
  const heading =
    job.status === "queued"
      ? "Account backfill queued"
      : job.status === "retrying"
        ? "Account backfill rate limited"
        : job.status === "paused"
          ? "Account backfill paused"
          : job.status === "completed"
            ? "Account backfill completed"
            : job.status === "resolving"
              ? "Fetching account identity"
              : repository
                ? "Fetching public account records"
                : "Fetching public account associations";
  const statusCopy =
    job.status === "queued"
      ? waitingForDependency
        ? "Waiting for the account repository fetch to finish before this job can start."
        : "Waiting for available backfill worker capacity."
      : job.status === "retrying"
        ? "The upstream service rate limited this job. It will resume automatically."
        : job.status === "paused"
          ? "This job is paused and will not make progress until it is resumed."
          : job.status === "completed"
            ? "The public records collected by this job are now available to search."
            : "This job is fetching public data now.";
  return (
    <section className="rounded-xl border border-sky-500/30 bg-sky-500/[.06] p-5">
      <div className="flex flex-wrap items-start gap-4">
        <div className="grid size-10 place-items-center rounded-xl bg-sky-500/10">
          <DatabaseZap
            className={`size-5 text-sky-400 ${activeStatuses.has(job.status) ? "animate-pulse" : ""}`}
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="font-medium">
              {heading}
            </h2>
            <span className="rounded-md border border-sky-500/20 px-2 py-0.5 text-[10px] uppercase tracking-wide text-sky-300">
              {label}
            </span>
          </div>
          {(job.handle || job.did) && (
            <div className="mt-2">
              {job.did ? (
                <Link
                  to="/accounts/$did"
                  params={{ did: job.did }}
                  className="text-sm text-sky-300 hover:text-sky-200"
                >
                  {job.handle ? <><span className="block">@{job.handle}</span><span className="block break-all font-mono text-[9px] text-dim">{job.did}</span></> : <>{job.did} <span className="text-[9px] text-dim">resolving…</span></>} →
                </Link>
              ) : (
                <span className="text-sm text-sky-300">@{job.handle}</span>
              )}
            </div>
          )}
          <p className="mt-3 text-xs leading-5 text-dim">
            {statusCopy} This job runs durably in the background, so you can
            leave this page safely.
          </p>
          <div className="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-xs">
            {job.status === "queued" && job.records_written === 0 ? (
              <span>No records written yet</span>
            ) : (
              <span>
                <b className="font-mono text-white">{job.records_written}</b>{" "}
                records written
              </span>
            )}
            {job.retry_count > 0 && <span>{job.retry_count} retries</span>}
            {waiting && (
              <span className="inline-flex items-center gap-1 text-sky-200">
                <Clock3 className="size-3" />
                Rate limited · resumes {retryAt.toLocaleTimeString()}
              </span>
            )}
          </div>
          {job.last_error && (
            <p className="mt-3 rounded-lg bg-red-500/10 p-3 text-xs text-red-200">
              {job.last_error}
            </p>
          )}
        </div>
      </div>
    </section>
  );
}

export function SearchPage({
  initialQuery,
  initialType,
  initialOffset,
}: {
  initialQuery: string;
  initialType: SearchKind;
  initialOffset: number;
}) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [input, setInput] = useState(initialQuery);
  useEffect(() => setInput(initialQuery), [initialQuery]);
  const query = initialQuery.trim();
  const offset = Math.max(0, initialOffset);
  const results = useQuery({
    queryKey: ["search", query, initialType, offset],
    queryFn: () =>
      api<SearchResponse>(
        `/api/search?q=${encodeURIComponent(query)}&type=${initialType}&limit=${PAGE_SIZE}&offset=${offset}`,
      ),
    enabled: query.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
  const recentJobs = useQuery({
    queryKey: ["backfills", "recent"],
    queryFn: () => api<BackfillJobsResponse>("/api/backfills?limit=10"),
    staleTime: 10_000,
    refetchInterval: (query) =>
      query.state.data?.jobs.some((job) => activeStatuses.has(job.status))
        ? 5_000
        : false,
  });
  const refreshSearch = useCallback(() => {
    void queryClient.invalidateQueries({
      queryKey: ["search", query, initialType, offset],
    });
  }, [initialType, offset, query, queryClient]);
  const go = (next: { q?: string; type?: SearchKind; offset?: number }) =>
    navigate({
      to: "/search",
      search: {
        q: next.q ?? query,
        type: next.type ?? initialType,
        offset: next.offset ?? 0,
      },
    });
  return (
    <section className="mx-auto max-w-5xl">
      <div className="mb-7">
        <p className="eyebrow">Retained public data</p>
        <h1 className="mt-2 text-3xl font-semibold tracking-tight">Search the open network</h1>
        <p className="mt-2 text-sm text-dim">
          Find a person by handle or DID, open a post by its AT URI, or search
          recently retained public posts. This is an explorer of collected data—not a complete index of ATProto.
        </p>
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          go({ q: input.trim(), offset: 0 });
        }}
        className="flex gap-2"
      >
        <label className="relative min-w-0 flex-1">
          <Search className="absolute left-4 top-1/2 size-4 -translate-y-1/2 text-dim" />
          <input
            autoFocus
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder="Search handles, DIDs, post text, URLs, or AT URIs…"
            className="h-12 w-full rounded-xl border border-line bg-panel pl-11 pr-4 text-sm outline-none focus:border-sky-500/60"
          />
        </label>
        <button className="rounded-xl bg-sky-500 px-5 text-sm font-semibold text-black hover:bg-sky-400">
          Search
        </button>
      </form>
      <div className="mt-3 flex flex-wrap gap-2">
        {kinds.map((kind) => (
          <button
            key={kind.value}
            onClick={() => go({ type: kind.value, offset: 0 })}
            className={`rounded-lg border px-3 py-1.5 text-xs ${initialType === kind.value ? "border-sky-500 bg-sky-500/15 text-sky-300" : "border-line text-dim hover:text-white"}`}
          >
            {kind.label}
          </button>
        ))}
      </div>
      <p className="mt-3 text-xs leading-6 text-dim">Text, domain and collection search covers the last 24 hours. Exact DIDs and AT URIs search retained history. For arbitrary record fields, browse <Link to="/collections" search={{collection:''}} className="text-cyan">collections</Link> and inspect the record JSON.</p>
      <div
        className={`mt-7 space-y-3 transition-opacity ${results.isPlaceholderData ? "opacity-50" : ""}`}
      >
        {!query && (
          <div className="grid gap-4 sm:grid-cols-3">
            {[
              ['People', 'Enter a handle such as bsky.app, or paste a DID.', 'bsky.app', 'account'],
              ['Conversations', 'Find recent public posts containing a word or phrase.', 'hello', 'record'],
              ['Links', 'See what people are sharing from a particular domain.', 'wikipedia.org', 'record'],
            ].map(([title, description, q, type]) => <button key={title} onClick={() => { setInput(q); go({ q, type: type as SearchKind }) }} className="panel p-6 text-left transition hover:border-cyan/50"><h2 className="font-medium text-white">{title} →</h2><p className="mt-3 text-sm leading-6 text-dim">{description}</p></button>)}
          </div>
        )}
        {results.isLoading && (
          <div className="panel flex min-h-56 items-center justify-center text-sm text-dim">
            Searching…
          </div>
        )}
        {results.isError && (
          <div className="rounded-xl border border-red-500/30 bg-red-500/5 p-5 text-sm text-red-200">
            This search couldn’t finish. Try an exact handle, DID, post URI or more specific phrase. <button onClick={() => void results.refetch()} className="ml-2 underline">Retry</button>
          </div>
        )}
        {results.data?.backfill_job && (
          <BackfillState
            initial={results.data.backfill_job}
            onCompleted={refreshSearch}
          />
        )}{" "}
        {results.data?.results.map((item, index) => (
          <Result
            key={`${item.result_type}:${item.did}:${item.at_uri || item.event_id || index}`}
            item={item}
          />
        ))}
        {results.data &&
          !results.data.results.length &&
          !results.data.backfill_job && (
            <div className="panel flex min-h-56 items-center justify-center text-sm text-dim">
              No retained results matched this search.
            </div>
          )}
      </div>
      {results.data && results.data.results.length > 0 && (
        <div className="mt-5 flex items-center justify-between text-xs text-dim">
          <span>
            Showing {offset + 1}–{offset + results.data.results.length}
          </span>
          <div className="flex gap-2">
            <button
              disabled={offset === 0 || results.isPlaceholderData}
              onClick={() => go({ offset: Math.max(0, offset - PAGE_SIZE) })}
              className="inline-flex items-center gap-1 rounded-lg border border-line px-3 py-2 text-white disabled:opacity-30"
            >
              <ArrowLeft className="size-3.5" /> Previous
            </button>
            <button
              disabled={!results.data.has_more || results.isPlaceholderData}
              onClick={() => go({ offset: offset + PAGE_SIZE })}
              className="inline-flex items-center gap-1 rounded-lg border border-line px-3 py-2 text-white disabled:opacity-30"
            >
              Next <ArrowRight className="size-3.5" />
            </button>
          </div>
        </div>
      )}
      {recentJobs.data?.jobs.length ? (
        <details className="panel mt-10 p-5">
          <summary className="cursor-pointer text-sm font-medium">
            Recent repository fetches{" "}
            <span className="ml-2 text-xs font-normal text-dim">
              {recentJobs.data.jobs.length}
            </span>
          </summary>
          <div className="mt-4 space-y-3 text-xs">
            {recentJobs.data.jobs.map((job) => (
              <div
                key={job.id}
                className="rounded-lg border border-line px-3 py-3"
              >
                <BackfillState initial={job} compact />
              </div>
            ))}
          </div>
        </details>
      ) : null}
    </section>
  );
}
