import { lazy, Suspense, useMemo, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
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
import {
  Activity,
  ArrowDownUp,
  Bot,
  ChevronRight,
  CircleAlert,
  Gauge,
  Network,
  Search,
  TrendingUp,
} from "lucide-react";
import { Link, useNavigate } from "@tanstack/react-router";
import { Chart } from "@tanstack/charts/react";
import { defineChart, lineY } from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { tooltip } from "@tanstack/charts/tooltip";
import { api } from "./api";
import { LiveUpdates } from "./live";
const RecordPage = lazy(() => import('./RecordDrawer').then(m => ({ default: m.RecordPage })));
import { FirehoseView } from "./FirehoseView";
const FirehoseImagesPage = lazy(() => import('./FirehoseImagesPage').then(m => ({ default: m.FirehoseImagesPage })));
const TrendPage = lazy(() => import('./TrendPage').then(m => ({ default: m.TrendPage })));
import { CommandPalette } from "./CommandPalette";
const SearchPage = lazy(() => import('./SearchPage').then(m => ({ default: m.SearchPage })));
const AccountPage = lazy(() => import('./AccountPage').then(m => ({ default: m.AccountPage })));
import { TopAccounts } from "./TopAccounts";
import { OverviewPage } from "./OverviewPage";
import { BackfillDetailPage, BackfillsPage } from "./BackfillsPage";
import { IdentitiesPage, IdentityDetailPage } from "./IdentitiesPage";
import { CrawlDetailPage } from "./CrawlsPage";
const CoveragePage = lazy(() => import('./CoveragePage').then(m => ({ default: m.CoveragePage })));
import { CommunitiesPage, CommunityDetailPage } from "./CommunitiesPage";
import type {
  Anomalies,
  ClusterDetail,
  Detection,
  Identity,
  Rates,
  SearchKind,
  StorageStats,
  SystemSample,
  SystemStats,
  TrendingItem,
} from "./types";

const n = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
const compact = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});
const bytes = (value?: number) => {
  if (value == null || !Number.isFinite(value)) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let i = 0;
  let x = Math.max(0, value);
  while (x >= 1024 && i < units.length - 1) {
    x /= 1024;
    i++;
  }
  return `${x.toLocaleString(undefined, { maximumFractionDigits: i ? 2 : 0 })} ${units[i]}`;
};
const duration = (seconds?: number) =>
  seconds == null
    ? "—"
    : seconds < 3600
      ? `${Math.floor(seconds / 60)}m`
      : seconds < 86400
        ? `${(seconds / 3600).toFixed(1)}h`
        : `${(seconds / 86400).toFixed(1)}d`;
const runway = (days?: number) =>
  days == null || !Number.isFinite(days)
    ? "Measuring growth…"
    : days < 2
      ? `${Math.max(0, days * 24).toFixed(1)} hours`
      : days < 90
        ? `${days.toFixed(1)} days`
        : days < 730
          ? `${(days / 30.4375).toFixed(1)} months`
          : `${(days / 365.25).toFixed(1)} years`;
const limitLabel = (storage?: StorageStats) =>
  storage?.limiting_factor === "observatory_allocation"
    ? "Allocation fills first"
    : storage?.limiting_factor === "physical_array"
      ? "Physical array fills first"
      : "Runway measuring";
const clock = (value: number) =>
  new Date(value).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
const observedTime = (value: number) => {
  const observed = new Date(value);
  const today = new Date();
  const sameDay =
    observed.getFullYear() === today.getFullYear() &&
    observed.getMonth() === today.getMonth() &&
    observed.getDate() === today.getDate();
  return sameDay
    ? clock(value)
    : observed.toLocaleString([], {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
};
const pct = (x?: number) =>
  x == null ? "—" : `${(x * (x <= 1 ? 100 : 1)).toFixed(1)}%`;
const isDid = (value?: string) => Boolean(value?.startsWith("did:"));

function AccountIdentity({
  did,
  identities,
  onOpen,
  compact = false,
}: {
  did: string;
  identities: Map<string, Identity>;
  onOpen: (did: string) => void;
  compact?: boolean;
}) {
  if (!isDid(did))
    return (
      <span className="min-w-0 truncate font-mono text-dim" title={did}>
        {did}
      </span>
    );
  const handle = identities.get(did)?.handle;
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onOpen(did);
      }}
      title={`${handle ? `@${handle} · ` : ""}${did}`}
      className="group min-w-0 text-left text-cyan hover:text-mint focus:outline-none focus-visible:ring-2 focus-visible:ring-mint/60"
    >
      <span className="block break-all font-medium">
        {handle ? `@${handle}` : did}
      </span>
      {handle && (
        <span className={`block break-all font-mono text-dim group-hover:text-dim/80 ${compact?'text-[9px]':'text-[10px]'}`}>
          {did}
        </span>
      )}
      {!handle&&<span className="block text-[9px] text-dim">resolving…</span>}
    </button>
  );
}

const didFromTarget = (target: string) =>
  target.startsWith("at://")
    ? target.slice(5).split("/")[0]
    : target.startsWith("did:")
      ? target
      : undefined;
function blueskyPostUrl(
  uri?: string,
  handles?: Map<string, string | undefined>,
) {
  if (!uri?.startsWith("at://")) return undefined;
  const [did, collection, rkey] = uri.slice(5).split("/");
  if (collection !== "app.bsky.feed.post" || !rkey) return undefined;
  return `https://bsky.app/profile/${encodeURIComponent(handles?.get(did) || did)}/post/${encodeURIComponent(rkey)}`;
}

function EvidenceLink({
  uri,
  label,
  onOpen,
}: {
  uri?: string;
  label: string;
  onOpen: (uri: string) => void;
}) {
  if (!blueskyPostUrl(uri)) return null;
  return (
    <button
      onClick={() => onOpen(uri!)}
      className="inline-flex items-center gap-1 text-cyan hover:text-mint"
    >
      {label}
      <ChevronRight className="size-3" />
    </button>
  );
}

function useApi<T>(
  key: string,
  path: string,
  enabled: boolean,
  interval: false | number = false,
) {
  return useQuery({
    queryKey: [key],
    queryFn: () => api<T>(path),
    enabled,
    refetchInterval: interval,
  });
}
function useIdentityMap(values: string[]) {
  const dids = [
    ...new Set(
      values
        .map((x) => didFromTarget(x))
        .filter((x): x is string => Boolean(x)),
    ),
  ].sort();
  const batches = Array.from({ length: Math.ceil(dids.length / 100) }, (_, i) =>
    dids.slice(i * 100, (i + 1) * 100),
  );
  const queries = useQueries({
    queries: batches.map((batch) => ({
      queryKey: ["identities", batch.join(",")],
      queryFn: () =>
        api<Identity[]>(
          `/api/identities?dids=${encodeURIComponent(batch.join(","))}`,
        ),
      staleTime: 300_000,
      gcTime: 3_600_000,
    })),
  });
  return new Map(
    queries
      .flatMap((q) => q.data || [])
      .map((identity) => [identity.did, identity.handle || undefined]),
  );
}
type GraphEdge = ClusterDetail["graph_edges"][number];

function InteractionGraph({
  edges,
  handles,
  onAccount,
}: {
  edges: GraphEdge[];
  handles: Map<string, string | undefined>;
  onAccount: (did: string) => void;
}) {
  const [hovered, setHovered] = useState<string>();
  const nodes = useMemo(() => {
    const weights = new Map<string, number>();
    edges.forEach((edge) => {
      weights.set(
        edge.source,
        (weights.get(edge.source) || 0) + edge.event_count,
      );
      weights.set(
        edge.target,
        (weights.get(edge.target) || 0) + edge.event_count,
      );
    });
    const ordered = [...weights].sort(
      (a, b) => b[1] - a[1] || a[0].localeCompare(b[0]),
    );
    const center = { x: 400, y: 225 },
      radius = Math.min(165, 80 + ordered.length * 7);
    return ordered.map(([id, weight], index) => {
      const angle = -Math.PI / 2 + (index / ordered.length) * Math.PI * 2;
      return {
        id,
        weight,
        x: center.x + Math.cos(angle) * radius,
        y: center.y + Math.sin(angle) * radius,
      };
    });
  }, [edges]);
  const positions = new Map(nodes.map((node) => [node.id, node]));
  const maxEdge = Math.max(...edges.map((edge) => edge.event_count), 1);
  const maxNode = Math.max(...nodes.map((node) => node.weight), 1);
  const label = (value: string) => {
    const did = didFromTarget(value);
    return did
      ? handles.get(did)
        ? `@${handles.get(did)}`
        : did
      : value;
  };
  const activeEdges = hovered
    ? edges.filter((edge) => edge.source === hovered || edge.target === hovered)
    : [];
  const activeWeight = activeEdges.reduce(
    (sum, edge) => sum + edge.event_count,
    0,
  );
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-black">
      <svg
        viewBox="0 0 800 450"
        className="block aspect-[16/9] min-h-72 w-full"
        role="img"
        aria-label={`Interaction network with ${nodes.length} accounts and ${edges.length} directed connections`}
      >
        <defs>
          <marker
            id="graph-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="5"
            markerHeight="5"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" fill="#58b8ff" />
          </marker>
        </defs>
        <g>
          {edges.map((edge, index) => {
            const source = positions.get(edge.source),
              target = positions.get(edge.target);
            if (!source || !target) return null;
            const selected =
              !hovered || edge.source === hovered || edge.target === hovered;
            return (
              <line
                key={`${edge.source}:${edge.target}:${index}`}
                x1={source.x}
                y1={source.y}
                x2={target.x}
                y2={target.y}
                stroke="#58b8ff"
                strokeWidth={1 + (5 * edge.event_count) / maxEdge}
                strokeOpacity={selected ? 0.72 : 0.09}
                markerEnd="url(#graph-arrow)"
                className="transition-opacity"
              />
            );
          })}
        </g>
        <g>
          {nodes.map((node) => {
            const did = didFromTarget(node.id),
              selected = hovered === node.id,
              connected =
                !hovered ||
                selected ||
                activeEdges.some(
                  (edge) => edge.source === node.id || edge.target === node.id,
                );
            const size = 8 + 10 * Math.sqrt(node.weight / maxNode);
            return (
              <g
                key={node.id}
                transform={`translate(${node.x} ${node.y})`}
                role={did ? "button" : undefined}
                tabIndex={did ? 0 : undefined}
                aria-label={`${label(node.id)}, ${node.weight} interactions`}
                onMouseEnter={() => setHovered(node.id)}
                onMouseLeave={() => setHovered(undefined)}
                onFocus={() => setHovered(node.id)}
                onBlur={() => setHovered(undefined)}
                onClick={() => did && onAccount(did)}
                onKeyDown={(event) => {
                  if (did && (event.key === "Enter" || event.key === " ")) {
                    event.preventDefault();
                    onAccount(did);
                  }
                }}
                className={did ? "cursor-pointer outline-none" : "outline-none"}
                opacity={connected ? 1 : 0.22}
              >
                <circle r={size + 7} fill="transparent" />
                <circle
                  r={size}
                  fill={selected ? "#8dd1ff" : "#160d06"}
                  stroke={selected ? "#fff2e5" : "#58b8ff"}
                  strokeWidth={selected ? 3 : 2}
                />
                <text
                  y={size + 16}
                  textAnchor="middle"
                  fill={selected ? "#fff2e5" : "#cbbeb3"}
                  fontSize="11"
                  fontFamily="ui-monospace, monospace"
                  paintOrder="stroke"
                  stroke="#000"
                  strokeWidth="4"
                >
                  {label(node.id)}
                </text>
                {did&&handles.get(did)&&<text y={size+25} textAnchor="middle" fill="#7f746b" fontSize="5.5" fontFamily="ui-monospace, monospace">{did}</text>}
              </g>
            );
          })}
        </g>
      </svg>
      <div className="flex min-h-11 flex-wrap items-center gap-x-5 gap-y-2 border-t border-line px-4 py-2 text-xs text-dim">
        <span>
          <i className="mr-1.5 inline-block size-2.5 rounded-full border-2 border-mint" />
          Account · size = interaction volume
        </span>
        <span>
          <i className="mr-1.5 inline-block h-0.5 w-5 bg-mint align-middle" />
          Directed edge · width = event count
        </span>
        {hovered && (
          <span className="ml-auto text-white">
            <b className="text-cyan">{label(hovered)}</b> ·{" "}
            {n.format(activeWeight)} connected events · click for account
          </span>
        )}
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  unit,
  icon: Icon,
  tone = "mint",
}: {
  label: string;
  value: string;
  unit?: string;
  icon: typeof Activity;
  tone?: "mint" | "cyan" | "amber";
}) {
  const color =
    tone === "cyan"
      ? "text-cyan"
      : tone === "amber"
        ? "text-amber-300"
        : "text-mint";
  return (
    <div className="panel p-5">
      <div className="flex items-center justify-between">
        <span className="eyebrow">{label}</span>
        <Icon className={`${color} size-4`} />
      </div>
      <div className="mt-4 flex items-baseline gap-2">
        <span className="text-3xl font-semibold tracking-tight">{value}</span>
        {unit && <span className="text-xs text-dim">{unit}</span>}
      </div>
    </div>
  );
}

function Panel({
  title,
  caption,
  children,
  className = "",
}: {
  title: string;
  caption?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel min-w-0 p-5 ${className}`}>
      <div className="mb-5">
        <h2 className="font-semibold tracking-tight">{title}</h2>
        {caption && <p className="mt-1 text-xs text-dim">{caption}</p>}
      </div>
      {children}
    </section>
  );
}

function Empty({
  children = "Waiting for enough activity…",
}: {
  children?: React.ReactNode;
}) {
  return (
    <div className="flex min-h-28 items-center justify-center text-sm text-dim">
      {children}
    </div>
  );
}

function RateChart({ rates }: { rates: Rates }) {
  const definition = useMemo(() => {
    const rows = rates.windows.map((w) => ({
      window: w.seconds / 60,
      rate: w.events_per_sec,
    }));
    return defineChart({
      marks: [
        lineY(rows, {
          id: "Event rate",
          x: "window",
          y: "rate",
          stroke: "#58b8ff",
          points: true,
        }),
      ],
      scales: {
        x: { scale: scaleLinear, axis: { label: "Window (minutes)" } },
        y: { scale: scaleLinear, axis: { label: "Events / second" } },
      },
      focus: "nearest-x",
      maxFocusDistance: Number.POSITIVE_INFINITY,
      tooltip: {
        use: tooltip,
        items: [
          {
            channel: "x",
            label: "Window",
            text: (p) => `${n.format(Number(p.xValue))} min`,
          },
          {
            channel: "y",
            label: "Event rate",
            text: (p) => `${n.format(Number(p.yValue))} events/s`,
          },
        ],
      },
    });
  }, [rates]);
  return (
    <Chart
      definition={definition}
      height={150}
      ariaLabel="Events per second by analysis window"
    />
  );
}

function RankList({
  items,
  onOpen,
}: {
  items: TrendingItem[];
  onOpen?: (item: TrendingItem) => void;
}) {
  if (!items.length) return <Empty />;
  const max = Math.max(...items.map((x) => x.count), 1);
  const open = (item: TrendingItem) =>
    onOpen?.(item) ??
    (item.sample && item.key.startsWith("at://")
      ? window.location.assign(`/records/${encodeURIComponent(item.key)}`)
      : undefined);
  return (
    <div className="space-y-4">
      {items.slice(0, 8).map((item, i) => (
        <button
          type="button"
          disabled={!onOpen && !item.sample}
          onClick={() => open(item)}
          className="block w-full text-left disabled:cursor-default"
          key={item.key}
        >
          <div className="mb-1.5 flex items-center gap-3 text-sm">
            <span className="w-5 text-xs text-dim">
              {String(i + 1).padStart(2, "0")}
            </span>
            <span
              className="min-w-0 flex-1 truncate hover:text-sky-300"
              title={item.key}
            >
              {item.sample || item.key}
            </span>
            <span className="font-mono text-xs text-mint">
              {compact.format(item.count)}
            </span>
          </div>
          <div className="ml-8 h-1 overflow-hidden rounded bg-line">
            <div
              className="h-full rounded bg-mint/70"
              style={{ width: `${(item.count / max) * 100}%` }}
            />
          </div>
        </button>
      ))}
    </div>
  );
}

function ConversationList({
  items,
  onRecord,
}: {
  items: TrendingItem[];
  onRecord: (uri: string) => void;
}) {
  if (!items.length) return <Empty />;
  return (
    <div className="grid gap-3 lg:grid-cols-2">
      {items.slice(0, 8).map((item) => {
        const record = item.record;
        const uri = record?.at_uri || item.key;
        return (
          <button
            type="button"
            key={item.key}
            onClick={() => onRecord(uri)}
            className="min-w-0 rounded-xl border border-line bg-black p-4 text-left transition hover:border-sky-500/40"
          >
            <div className="flex items-center gap-2 text-xs">
              <span className="min-w-0 flex-1 break-all font-medium text-sky-300">
                {item.handle ? <><span className="block">@{item.handle}</span>{record&&<span className="block break-all font-mono text-[9px] text-dim">{record.actor_did}</span>}</> : record ? <>{record.actor_did}<span className="ml-2 text-[9px] text-dim">resolving…</span></> : "Observed conversation"}
              </span>
              <span className="shrink-0 rounded-md bg-sky-500/10 px-2 py-1 font-mono text-sky-300">
                {compact.format(item.count)} replies
              </span>
            </div>
            <p className="mt-3 line-clamp-3 min-h-10 whitespace-pre-wrap text-sm leading-5 text-white/85">
              {record?.text ||
                item.sample ||
                "Open the retained conversation record"}
            </p>
            <div className="mt-3 flex items-center justify-between gap-3 text-[10px] text-dim">
              {record?.timestamp_ms ? (
                <time>{new Date(record.timestamp_ms).toLocaleString()}</time>
              ) : (
                <span>Retained conversation</span>
              )}
              <span className="truncate text-sky-300">
                View conversation →
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}

const clusterHelper = createColumnHelper<Detection>();
const clusterColumns = [
  clusterHelper.accessor("cluster_id", {
    header: "Cluster",
    cell: (x) => (
      <span className="font-mono text-xs text-cyan">
        {x.getValue().replace("coord-", "")}
      </span>
    ),
  }),
  clusterHelper.accessor((x) => x.accounts.length, {
    id: "accounts",
    header: "Accounts",
  }),
  clusterHelper.accessor("event_count", { header: "Events" }),
  clusterHelper.accessor((x) => x.scores.coordination_score, {
    id: "score",
    header: "Coordination",
    cell: (x) => <Score value={x.getValue()} />,
  }),
  clusterHelper.accessor(
    (x) => x.common_domains[0] || x.common_targets[0] || "—",
    {
      id: "evidence",
      header: "Primary evidence",
      cell: (x) => (
        <span className="block max-w-60 truncate text-dim">{x.getValue()}</span>
      ),
    },
  ),
];

function Score({ value }: { value: number }) {
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-16 rounded bg-line">
        <div
          className="h-full rounded bg-cyan"
          style={{ width: `${value * 100}%` }}
        />
      </div>
      <span className="font-mono text-xs">{value.toFixed(2)}</span>
    </div>
  );
}

function ClusterTable({
  data,
  onOpen,
}: {
  data: Detection[];
  onOpen: (x: Detection) => void;
}) {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "score", desc: true },
  ]);
  const [filter, setFilter] = useState("");
  const table = useReactTable({
    data,
    columns: clusterColumns,
    state: { sorting, globalFilter: filter },
    initialState: { pagination: { pageIndex: 0, pageSize: 10 } },
    onSortingChange: setSorting,
    onGlobalFilterChange: setFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
  });
  return (
    <>
      <div className="relative mb-4 max-w-xs">
        <Search className="absolute left-3 top-2.5 size-4 text-dim" />
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter clusters…"
          className="w-full rounded-lg border border-line bg-ink/60 py-2 pl-9 pr-3 text-sm outline-none focus:border-mint/60"
        />
      </div>
      <div className="scrollbar overflow-x-auto">
        <table className="w-full min-w-180 text-left text-sm">
          <thead className="border-b border-line text-xs text-dim">
            {table.getHeaderGroups().map((h) => (
              <tr key={h.id}>
                {h.headers.map((x) => (
                  <th key={x.id} className="px-3 py-3 font-medium">
                    <button
                      className="flex items-center gap-1"
                      onClick={x.column.getToggleSortingHandler()}
                    >
                      {flexRender(x.column.columnDef.header, x.getContext())}
                      {x.column.getCanSort() && (
                        <ArrowDownUp className="size-3" />
                      )}
                    </button>
                  </th>
                ))}
                <th />
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.map((row) => (
              <tr
                key={row.id}
                onClick={() => onOpen(row.original)}
                className="cursor-pointer border-b border-line/60 hover:bg-white/[.025]"
              >
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id} className="px-3 py-3.5">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
                <td>
                  <ChevronRight className="size-4 text-dim" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!data.length && <Empty>No active coordination clusters</Empty>}
      </div>
      {data.length > 0 && (
        <div className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs text-dim">
          <span>
            Page {table.getState().pagination.pageIndex + 1} of{" "}
            {table.getPageCount()}
          </span>
          <div className="flex items-center gap-2">
            <label>
              Rows{" "}
              <select
                value={table.getState().pagination.pageSize}
                onChange={(e) => table.setPageSize(Number(e.target.value))}
                className="rounded border border-line bg-ink px-2 py-1.5 text-white"
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
      )}
    </>
  );
}

function ClusterPage({
  clusterId,
  onAccount,
  onRecord,
}: {
  clusterId: string;
  onAccount: (did: string) => void;
  onRecord: (uri: string) => void;
}) {
  const {
    data: d,
    isLoading,
    isError,
  } = useQuery({
    queryKey: ["cluster", clusterId],
    queryFn: () =>
      api<ClusterDetail>(`/api/clusters/${encodeURIComponent(clusterId)}`),
  });
  const identityValues = d
    ? [
        ...d.accounts,
        ...d.common_targets,
        ...d.sample_events.flatMap((x) => [
          x.actor_did,
          x.at_uri || "",
          x.target || "",
          x.reply_parent || "",
          x.reply_root || "",
          x.quote_target || "",
        ]),
        ...d.graph_edges.flatMap((x) => [x.source, x.target]),
      ]
    : [];
  const handles = useIdentityMap(identityValues);
  if (isLoading) return <Empty>Loading detection…</Empty>;
  if (isError || !d) return <Empty>Detection not found</Empty>;
  const graphEdges = d.graph_edges.filter((x) => x.source !== x.target);
  const selfEdges = d.graph_edges.length - graphEdges.length;
  return (
    <section className="panel mx-auto max-w-5xl p-6">
      <p className="eyebrow">Detection evidence</p>
      <h2 className="mt-2 font-mono text-xl text-cyan">{d.cluster_id}</h2>
      <p className="mt-2 text-xs text-dim">
        Observed {new Date(d.first_seen).toLocaleString()} —{" "}
        {new Date(d.last_seen).toLocaleString()}
      </p>
      <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-5">
        {Object.entries(d.scores).map(([k, v]) => (
          <div className="rounded-xl border border-line p-3" key={k}>
            <div className="text-lg font-semibold">{v.toFixed(2)}</div>
            <div className="mt-1 text-[10px] uppercase tracking-wide text-dim">
              {k.replaceAll("_", " ")}
            </div>
          </div>
        ))}
      </div>
      <h3 className="eyebrow mt-8 mb-3">
        Participating identities · {d.accounts.length}
      </h3>
      <div className="flex flex-wrap gap-2">
        {d.accounts.map((x) => (
          <div
            key={x}
            className="max-w-full rounded-xl border border-line bg-panel px-3 py-2 text-xs"
          >
            <AccountIdentity
              did={x}
              identities={
                new Map([
                  [
                    x,
                    {
                      did: x,
                      handle: handles.get(x),
                      observed_at: "",
                      source: "",
                    },
                  ],
                ])
              }
              onOpen={onAccount}
            />
          </div>
        ))}
      </div>
      <div className="mt-8 grid gap-5 sm:grid-cols-2">
        <div>
          <h3 className="eyebrow mb-3">Evidence summary</h3>
          <dl className="space-y-2 text-sm">
            <Row k="Shared target ratio" v={pct(d.shared_target_ratio)} />
            <Row
              k="Median timing"
              v={`${n.format(d.median_timing_difference_ms)} ms`}
            />
            <Row k="Repeated content" v={n.format(d.repeated_content_count)} />
            <Row k="Events" v={n.format(d.event_count)} />
          </dl>
        </div>
        <div>
          <h3 className="eyebrow mb-3">Common domains</h3>
          {d.common_domains.length ? (
            <div className="flex flex-wrap gap-2">
              {d.common_domains.map((x) => (
                <span
                  className="rounded bg-mint/10 px-2 py-1 text-xs text-mint"
                  key={x}
                >
                  {x}
                </span>
              ))}
            </div>
          ) : (
            <span className="text-sm text-dim">None</span>
          )}
        </div>
      </div>
      <h3 className="eyebrow mt-8 mb-3">Interaction graph</h3>
      {graphEdges.length ? (
        <>
          <InteractionGraph
            edges={graphEdges}
            handles={handles}
            onAccount={onAccount}
          />
          {selfEdges > 0 && (
            <p className="mt-3 text-xs text-dim">
              {selfEdges} self-interaction {selfEdges === 1 ? "edge" : "edges"}{" "}
              omitted from the coordination graph.
            </p>
          )}
        </>
      ) : (
        <Empty>
          {selfEdges
            ? "Only self-interactions were retained; these are not coordination evidence."
            : "No directed edges retained"}
        </Empty>
      )}
      <h3 className="eyebrow mt-8 mb-3">Sample events</h3>
      <div className="space-y-2">
        {d.sample_events.map((x) => (
          <article
            key={x.event_id}
            className="rounded-lg border border-line p-3 text-xs"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="uppercase text-mint">{x.kind}</span>
              <button
                onClick={() => onAccount(x.actor_did)}
                className="break-all text-left font-medium text-cyan hover:text-mint"
              >
                {handles.get(x.actor_did)
                  ? `@${handles.get(x.actor_did)}`
                  : x.actor_did}
              </button>
              {handles.get(x.actor_did)&&<span className="break-all font-mono text-[9px] text-dim">{x.actor_did}</span>}
              <span className="text-dim">
                · {new Date(x.timestamp).toLocaleTimeString()}
              </span>
              <span className="ml-auto">
                <EvidenceLink
                  uri={x.at_uri || x.target}
                  label="View post"
                  onOpen={onRecord}
                />
              </span>
            </div>
            {x.text && (
              <p className="mt-2 line-clamp-3 leading-relaxed text-white/80">
                {x.text}
              </p>
            )}
            <div className="mt-2 flex flex-wrap gap-3">
              <EvidenceLink uri={x.target} label="Target" onOpen={onRecord} />
              <EvidenceLink
                uri={x.reply_parent}
                label="Reply parent"
                onOpen={onRecord}
              />
              <EvidenceLink
                uri={x.reply_root}
                label="Thread root"
                onOpen={onRecord}
              />
              <EvidenceLink
                uri={x.quote_target}
                label="Quoted post"
                onOpen={onRecord}
              />
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-4">
      <dt className="text-dim">{k}</dt>
      <dd className="font-mono">{v}</dd>
    </div>
  );
}

function SystemHistoryChart({ history }: { history: SystemSample[] }) {
  const definition = useMemo(() => {
    const rows = history.map((x) => ({
      time: x.timestamp,
      rate: x.events_per_second,
    }));
    return defineChart({
      marks: [
        lineY(rows, {
          id: "Event throughput",
          x: "time",
          y: "rate",
          stroke: "#8dd1ff",
          points: true,
        }),
      ],
      scales: {
        x: {
          scale: scaleLinear,
          axis: {
            label: "Local time",
            ticks: { format: (v: number) => observedTime(v) },
          },
        },
        y: {
          scale: scaleLinear,
          axis: {
            label: "Events / second",
            ticks: { format: (v: number) => compact.format(v) },
          },
        },
      },
      focus: "nearest-x",
      maxFocusDistance: Number.POSITIVE_INFINITY,
      tooltip: {
        use: tooltip,
        items: [
          {
            channel: "x",
            label: "Observed",
            text: (p) => observedTime(Number(p.xValue)),
          },
          {
            channel: "y",
            label: "Throughput",
            text: (p) => `${compact.format(Number(p.yValue))} events/s`,
          },
        ],
      },
    });
  }, [history]);
  return history.length > 1 ? (
    <Chart
      definition={definition}
      height={170}
      ariaLabel="Live event throughput history"
    />
  ) : (
    <Empty>Collecting history samples…</Empty>
  );
}

function CpuHistoryChart({ history }: { history: SystemSample[] }) {
  const definition = useMemo(() => {
    const rows = history
      .filter((x) => x.host_cpu_utilization_percent != null)
      .map((x) => ({
        time: x.timestamp,
        host: x.host_cpu_utilization_percent || 0,
        process: x.process_cpu_utilization_percent || 0,
        container: x.container_cpu_utilization_percent || 0,
      }));
    return defineChart({
      marks: [
        lineY(rows, {
          id: "Host CPU",
          x: "time",
          y: "host",
          stroke: "#58b8ff",
          points: true,
        }),
        lineY(rows, {
          id: "Container CPU",
          x: "time",
          y: "container",
          stroke: "#8dd1ff",
          points: true,
        }),
        lineY(rows, {
          id: "Rust process CPU",
          x: "time",
          y: "process",
          stroke: "#c4e6ff",
          points: true,
        }),
      ],
      scales: {
        x: {
          scale: scaleLinear,
          axis: {
            label: "Local time",
            ticks: { format: (v: number) => observedTime(v) },
          },
        },
        y: {
          scale: scaleLinear,
          axis: {
            label: "CPU utilization",
            ticks: { format: (v: number) => `${n.format(v)}%` },
          },
        },
      },
      focus: "group-x",
      maxFocusDistance: Number.POSITIVE_INFINITY,
      tooltip: {
        use: tooltip,
        items: [
          {
            channel: "x",
            label: "Observed",
            text: (p) => observedTime(Number(p.xValue)),
          },
          {
            channel: "y",
            label: "CPU utilization",
            text: (p) => `${n.format(Number(p.yValue))}%`,
          },
        ],
      },
    });
  }, [history]);
  return history.filter((x) => x.host_cpu_utilization_percent != null).length >
    1 ? (
    <>
      <Chart
        definition={definition}
        height={170}
        ariaLabel="Host, container and process CPU utilization history"
      />
      <Legend
        labels={[
          ["Host", "#58b8ff"],
          ["Container", "#8dd1ff"],
          ["Process", "#c4e6ff"],
        ]}
      />
    </>
  ) : (
    <Empty>Collecting CPU samples…</Empty>
  );
}

function MemoryHistoryChart({ history }: { history: SystemSample[] }) {
  const definition = useMemo(() => {
    const rows = history.map((x) => ({
      time: x.timestamp,
      process: x.process_resident_memory_bytes,
      container: x.container_memory_used_bytes || 0,
    }));
    return defineChart({
      marks: [
        lineY(rows, {
          id: "Container memory",
          x: "time",
          y: "container",
          stroke: "#8dd1ff",
          points: true,
        }),
        lineY(rows, {
          id: "Process resident memory",
          x: "time",
          y: "process",
          stroke: "#c4e6ff",
          points: true,
        }),
      ],
      scales: {
        x: {
          scale: scaleLinear,
          axis: {
            label: "Local time",
            ticks: { format: (v: number) => observedTime(v) },
          },
        },
        y: {
          scale: scaleLinear,
          axis: { label: "Memory", ticks: { format: (v: number) => bytes(v) } },
        },
      },
      focus: "group-x",
      maxFocusDistance: Number.POSITIVE_INFINITY,
      tooltip: {
        use: tooltip,
        items: [
          {
            channel: "x",
            label: "Observed",
            text: (p) => observedTime(Number(p.xValue)),
          },
          {
            channel: "y",
            label: "Memory",
            text: (p) => bytes(Number(p.yValue)),
          },
        ],
      },
    });
  }, [history]);
  return history.length > 1 ? (
    <>
      <Chart
        definition={definition}
        height={170}
        ariaLabel="Container and process memory history"
      />
      <Legend
        labels={[
          ["Container memory", "#8dd1ff"],
          ["Process resident memory", "#c4e6ff"],
        ]}
      />
    </>
  ) : (
    <Empty>Collecting memory samples…</Empty>
  );
}

function Legend({ labels }: { labels: [string, string][] }) {
  return (
    <div className="mt-2 flex flex-wrap justify-center gap-4 text-[10px] uppercase tracking-wide text-dim">
      {labels.map(([label, color]) => (
        <span className="flex items-center gap-1.5" key={label}>
          <i className="size-2 rounded-full" style={{ background: color }} />
          {label}
        </span>
      ))}
    </div>
  );
}

function SystemView({
  storage,
  system,
  history,
}: {
  storage?: StorageStats;
  system?: SystemStats;
  history: SystemSample[];
}) {
  const c = system?.clickhouse,
    tables = c?.tables || [],
    latest = history.at(-1);
  const chRows = tables.reduce((a, x) => a + x.rows, 0),
    chParts = tables.reduce((a, x) => a + x.active_parts, 0),
    chCompressed = tables.reduce((a, x) => a + x.compressed_bytes, 0),
    chRaw = tables.reduce((a, x) => a + x.uncompressed_bytes, 0);
  return (
    <div className="grid gap-5 lg:grid-cols-3">
      <Panel
        title="Storage footprint"
        caption="Binary units; measured from live disk usage"
      >
        <dl className="space-y-3 text-sm">
          <Row k="Raw event archive" v={bytes(storage?.raw_archive_bytes)} />
          <Row k="ClickHouse" v={bytes(storage?.clickhouse_bytes)} />
          <Row k="Derived detections" v={bytes(storage?.derived_bytes)} />
          <Row
            k="Used / allocation"
            v={`${bytes(storage?.used_footprint_bytes)} / ${bytes(storage?.storage_budget_bytes)}`}
          />
          <Row
            k="Allocation remaining"
            v={bytes(storage?.remaining_allocation_bytes)}
          />
          <Row k="Daily growth" v={bytes(storage?.bytes_ingested_per_day)} />
          <Row
            k="Allocation runway"
            v={runway(storage?.allocation_runway_days)}
          />
          <Row k="Array runway" v={runway(storage?.physical_runway_days)} />
          <Row k="Limiting factor" v={limitLabel(storage)} />
        </dl>
      </Panel>
      <Panel
        title="Storage array & runtime"
        caption={system?.storage_array.path || "Archive mount capacity"}
      >
        <dl className="space-y-3 text-sm">
          <Row
            k="Used / available"
            v={`${bytes(system?.storage_array.used_bytes)} / ${bytes(system?.storage_array.available_bytes)}`}
          />
          <Row
            k="Process / container memory"
            v={`${bytes(system?.runtime.process_resident_memory_bytes)} / ${bytes(system?.runtime.container_memory_used_bytes)}`}
          />
          <Row
            k="Host CPU"
            v={
              (system?.runtime.host_cpu_utilization_percent ??
                latest?.host_cpu_utilization_percent) == null
                ? "—"
                : `${n.format(system?.runtime.host_cpu_utilization_percent ?? latest!.host_cpu_utilization_percent!)}%`
            }
          />
          <Row
            k="Container CPU"
            v={
              (system?.runtime.container_cpu_utilization_percent ??
                latest?.container_cpu_utilization_percent) == null
                ? "—"
                : `${n.format(system?.runtime.container_cpu_utilization_percent ?? latest!.container_cpu_utilization_percent!)}%`
            }
          />
          <Row
            k="Process CPU"
            v={
              (system?.runtime.process_cpu_utilization_percent ??
                latest?.process_cpu_utilization_percent) == null
                ? "—"
                : `${n.format(system?.runtime.process_cpu_utilization_percent ?? latest!.process_cpu_utilization_percent!)}%`
            }
          />
          <Row
            k="Service uptime"
            v={duration(system?.runtime.process_uptime_seconds)}
          />
        </dl>
      </Panel>
      <Panel
        title="ClickHouse"
        caption={system?.clickhouse_error || "Analytical datastore activity"}
      >
        <dl className="space-y-3 text-sm">
          <Row k="Rows" v={c ? compact.format(chRows) : "—"} />
          <Row
            k="Parts / tables"
            v={c ? `${n.format(chParts)} / ${n.format(tables.length)}` : "—"}
          />
          <Row k="Compressed" v={bytes(c ? chCompressed : undefined)} />
          <Row k="Uncompressed" v={bytes(c ? chRaw : undefined)} />
          <Row
            k="Compression ratio"
            v={chCompressed ? `${n.format(chRaw / chCompressed)}×` : "—"}
          />
          <Row
            k="Queries / mutations"
            v={
              c
                ? `${n.format(c.active_queries)} / ${n.format(c.pending_mutations)}`
                : "—"
            }
          />
        </dl>
      </Panel>
      <Panel
        title="Throughput history"
        caption="Events/sec · 15-second samples"
        className="lg:col-span-3"
      >
        <SystemHistoryChart history={history} />
      </Panel>
      <Panel
        title="CPU utilization"
        caption="Host, cgroup, and Rust process percentages"
        className="lg:col-span-2"
      >
        <CpuHistoryChart history={history} />
      </Panel>
      <Panel
        title="Memory utilization"
        caption="Container usage and process RSS"
      >
        <MemoryHistoryChart history={history} />
      </Panel>
    </div>
  );
}

export type AppRoute =
  | { view: "network" | "diagnostics" | "firehose" | "firehose-images" | "anomalies" | "system" | "backfills" | "identities" | "coverage" | "communities" }
  | { view: "search"; query: string; searchType: SearchKind; offset: number }
  | { view: "backfill"; jobId: string }
  | { view: "identity"; jobId: string }
  | { view: "crawl"; crawlId: string }
  | { view: "community"; communityId: string }
  | { view: "account"; did: string }
  | { view: "record"; uri: string }
  | { view: "cluster"; clusterId: string }
  | { view: "trend"; dimension: string; trendKey: string };

export default function App({ route }: { route: AppRoute }) {
  const navigate = useNavigate();
  const openAccount = (did: string) =>
    navigate({ to: "/accounts/$did", params: { did } });
  const openRecord = (uri: string) =>
    navigate({ to: "/records/$uri", params: { uri } });
  const openCluster = (cluster: Detection) =>
    navigate({
      to: "/clusters/$clusterId",
      params: { clusterId: cluster.cluster_id },
    });
  const openTrend = (dimension: string, item: TrendingItem) =>
    navigate({
      to: "/trends/$dimension",
      params: { dimension },
      search: { key: item.key },
    });
  const network = route.view === "diagnostics",
    detections = route.view === "anomalies",
    systemPage = route.view === "system";
  const rates = useApi<Rates>("rates", "/api/stats", network);
  const ingestion = useApi<{ connected: boolean; lag_ms: number }>("ingestion", "/api/ingestion", true, 15000);
  const urls = useApi<[string, number][]>(
    "urls",
    "/api/trending/urls",
    network,
  );
  const domains = useApi<[string, number][]>(
    "domains",
    "/api/trending/domains",
    network,
  );
  const prints = useApi<TrendingItem[]>(
    "prints",
    "/api/trending/fingerprints",
    network,
  );
  const conversations = useApi<TrendingItem[]>(
    "conversations",
    "/api/trending/conversations",
    network,
  );
  const clusters = useApi<Detection[]>("clusters", "/api/clusters", detections);
  const anomalies = useApi<Anomalies>(
    "anomalies",
    "/api/anomalies",
    detections,
  );
  const storage = useApi<StorageStats>(
    "storage",
    "/api/storage",
    network || systemPage,
  );
  const system = useApi<SystemStats>(
    "system",
    "/api/system",
    systemPage,
    15000,
  );
  const history = useApi<SystemSample[]>(
    "system-history",
    "/api/system/history",
    systemPage,
    15000,
  );
  const anomalyHandles = useIdentityMap(
    (anomalies.data?.unusual_posting || []).map((x) => x.did),
  );
  const r = rates.data;
  const normalized = (x?: [string, number][]): TrendingItem[] =>
    (x || []).map(([key, count]) => ({
      key,
      count,
      recent_count: count,
      growth: 0,
    }));
  return (
    <>
      {(route.view === 'firehose' || route.view === 'firehose-images') && <LiveUpdates />}
      <header className="sticky top-0 z-30 border-b border-line/80 bg-ink/90 backdrop-blur-xl">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-5 py-4">
          <div className="flex items-center gap-3">
            <div className="grid size-9 place-items-center rounded-xl bg-mint text-ink">
              <Network className="size-5" />
            </div>
            <div>
              <h1 className="text-sm font-semibold tracking-tight">
                Bluesky Explorer
              </h1>
              <p className="text-[10px] uppercase tracking-[.16em] text-dim">
                A window into the public network
              </p>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <CommandPalette />
            <div className="flex items-center gap-2 text-xs text-dim">
              <span
                className={`size-2 rounded-full ${!ingestion.data?.connected || ingestion.isError ? "bg-red-400" : "bg-mint"}`}
              />
              {ingestion.isError ? "Ingestion status unavailable" : !ingestion.data ? "Checking ingestion…" : !ingestion.data.connected ? "Ingestion disconnected" : ingestion.data.lag_ms > 10000 ? "Catching up" : "Firehose live"}
            </div>
          </div>
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-4 py-6 sm:px-8 sm:py-8">
        <Suspense fallback={<div role="status" className="panel p-10 text-sm text-dim">Opening explorer…</div>}>
        <nav className="mb-7 flex gap-1 overflow-x-auto rounded-xl border border-line bg-panel/70 p-1 sm:w-fit">
          {(
            [
              ["/", "Bluesky now", "network"],
              ["/firehose", "Live stream", "firehose"],
              ["/search", "Search", "search"],
              ["/coverage", "Our dataset", "coverage"],
              ["/communities", "Communities", "communities"],
            ] as const
          ).map(([to, label, id]) => (
            <Link
              key={to}
              to={to}
              activeOptions={{ exact: true }}
              className={`shrink-0 rounded-lg px-4 py-2 text-xs font-medium transition ${route.view === id || (id === "firehose" && route.view === "firehose-images") ? "bg-mint text-ink" : "text-dim hover:text-white"}`}
            >
              {label}
            </Link>
          ))}
        </nav>
        <details className="mb-6 text-xs text-dim" open={['system','diagnostics','backfills','backfill','identities','identity','crawl','anomalies','cluster'].includes(route.view) || undefined}>
          <summary className="w-fit cursor-pointer rounded-lg border border-line px-3 py-2">Operations & investigations</summary>
          <div className="mt-3 flex flex-wrap gap-4">
            <Link to="/system">System health</Link><Link to="/system/network">Ingestion diagnostics</Link><Link to="/backfills">Backfills</Link><Link to="/identities">Identity queue</Link><Link to="/detections">Coordination investigations</Link>
          </div>
        </details>
        {route.view === "network" && <OverviewPage />}
        {route.view === "diagnostics" && (
          <>
            <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
              <Stat
                label="Events / sec"
                value={r ? n.format(r.events_per_sec) : "—"}
                icon={Activity}
              />
              <Stat
                label="Posts / sec"
                value={r ? n.format(r.posts_per_sec) : "—"}
                icon={TrendingUp}
                tone="cyan"
              />
              <Stat
                label="Likes / sec"
                value={r ? n.format(r.likes_per_sec) : "—"}
                icon={Gauge}
              />
              <Stat
                label="Follows / sec"
                value={r ? n.format(r.follows_per_sec) : "—"}
                icon={Network}
                tone="cyan"
              />
            </div>
            <div className="mt-5 grid gap-5 lg:grid-cols-3">
              <Panel
                title="Rate by window"
                caption="Events/sec across configured sliding windows"
                className="lg:col-span-2"
              >
                {r ? <RateChart rates={r} /> : <Empty />}
              </Panel>
              <Panel
                title="Storage now"
                caption="Archive and analytical footprint"
              >
                <div className="text-3xl font-semibold">
                  {bytes(
                    (storage.data?.raw_archive_bytes || 0) +
                      (storage.data?.clickhouse_bytes || 0),
                  )}
                </div>
                <div className="mt-5 space-y-2">
                  <Row
                    k="Used / allocation"
                    v={`${bytes(storage.data?.used_footprint_bytes)} / ${bytes(storage.data?.storage_budget_bytes)}`}
                  />
                  <Row
                    k="Allocation remaining"
                    v={bytes(storage.data?.remaining_allocation_bytes)}
                  />
                  <Row k="Archive" v={bytes(storage.data?.raw_archive_bytes)} />
                  <Row
                    k="ClickHouse"
                    v={bytes(storage.data?.clickhouse_bytes)}
                  />
                  <Row
                    k="Growth / day"
                    v={bytes(storage.data?.bytes_ingested_per_day)}
                  />
                  <Row
                    k="Projected runway"
                    v={runway(
                      storage.data?.limiting_factor === "physical_array"
                        ? storage.data?.physical_runway_days
                        : storage.data?.allocation_runway_days,
                    )}
                  />
                  <Row k="Limit" v={limitLabel(storage.data)} />
                </div>
              </Panel>
            </div>
            <TopAccounts />
            <div className="mt-5 grid gap-5 md:grid-cols-2 lg:grid-cols-3">
              <Panel
                title="Top domains"
                caption="Most shared in retained hot state"
              >
                <RankList
                  items={normalized(domains.data)}
                  onOpen={(item) => openTrend("domain", item)}
                />
              </Panel>
              <Panel title="Top URLs" caption="Shared destinations">
                <RankList
                  items={normalized(urls.data)}
                  onOpen={(item) => openTrend("url", item)}
                />
              </Panel>
              <Panel
                title="Text fingerprints"
                caption="Repeated normalized phrases"
              >
                <RankList
                  items={prints.data || []}
                  onOpen={(item) => openTrend("fingerprint", item)}
                />
              </Panel>
              <Panel
                title="Growing conversations"
                caption="Fastest recent reply growth"
                className="md:col-span-2 lg:col-span-3"
              >
                {conversations.isLoading ? (
                  <div className="grid gap-3 lg:grid-cols-2">
                    {[0, 1, 2, 3].map((x) => (
                      <div
                        key={x}
                        className="h-28 animate-pulse rounded-xl border border-line bg-white/[.025]"
                      />
                    ))}
                  </div>
                ) : (
                  <ConversationList
                    items={conversations.data || []}
                    onRecord={openRecord}
                  />
                )}
              </Panel>
            </div>
          </>
        )}
        {route.view === "anomalies" && (
          <>
            <div className="grid gap-4 sm:grid-cols-3">
              <Stat
                label="Active clusters"
                value={n.format(clusters.data?.length || 0)}
                icon={Network}
              />
              <Stat
                label="Posting anomalies"
                value={n.format(anomalies.data?.unusual_posting.length || 0)}
                icon={CircleAlert}
                tone="amber"
              />
              <Stat
                label="Content campaigns"
                value={n.format(
                  anomalies.data?.repeated_content_campaigns.length || 0,
                )}
                icon={Bot}
                tone="cyan"
              />
            </div>
            <Panel
              title="Coordination clusters"
              caption="Evidence-based behavioural signals — never identity labels"
              className="mt-5"
            >
              <ClusterTable data={clusters.data || []} onOpen={openCluster} />
            </Panel>
            <div className="mt-5 grid gap-5 lg:grid-cols-2">
              <Panel title="Unusual posting behaviour">
                {anomalies.data?.unusual_posting.length ? (
                  <div className="max-h-120 space-y-3 overflow-y-auto pr-1 scrollbar">
                    {anomalies.data.unusual_posting.map((a) => (
                      <button
                        onClick={() => openAccount(a.did)}
                        key={a.did}
                        className="block w-full rounded-xl border border-line p-3 text-left hover:border-mint/50"
                      >
                        <div className="flex justify-between gap-4">
                          <span
                            className="min-w-0 text-xs text-cyan"
                            title={a.did}
                          >
                            <b className="block break-all">
                              {anomalyHandles.get(a.did) || a.did}
                            </b>
                            {anomalyHandles.get(a.did) && (
                              <span className="block break-all font-mono text-[10px] text-dim">
                                {a.did}
                              </span>
                            )}
                          </span>
                          <b>{a.automation_score.toFixed(2)}</b>
                        </div>
                        <p className="mt-2 text-xs text-dim">
                          {a.events_24h} events · {a.posts_24h} posts ·{" "}
                          {a.unique_targets} targets
                        </p>
                      </button>
                    ))}
                  </div>
                ) : (
                  <Empty>No posting-rate anomalies</Empty>
                )}
              </Panel>
              <Panel title="Repeated-content campaigns">
                {anomalies.data?.repeated_content_campaigns.length ? (
                  <div className="max-h-120 space-y-3 overflow-y-auto pr-1 scrollbar">
                    {anomalies.data.repeated_content_campaigns.map((x) => (
                      <button
                        key={x.cluster_id}
                        onClick={() => openCluster(x)}
                        className="flex w-full justify-between rounded-xl border border-line p-3 text-left hover:border-cyan/50"
                      >
                        <span className="font-mono text-xs text-cyan">
                          {x.cluster_id}
                        </span>
                        <span className="text-xs">
                          {x.repeated_content_count} matches
                        </span>
                      </button>
                    ))}
                  </div>
                ) : (
                  <Empty>No repeated-content campaigns</Empty>
                )}
              </Panel>
            </div>
          </>
        )}
        {route.view === "firehose" && (
          <FirehoseView onAccount={openAccount} onRecord={openRecord} />
        )}
        {route.view === "firehose-images" && <FirehoseImagesPage onRecord={openRecord} />}
        {route.view === "search" && (
          <SearchPage
            initialQuery={route.query}
            initialType={route.searchType}
            initialOffset={route.offset}
          />
        )}
        {route.view === "coverage" && <CoveragePage />}
        {route.view === "communities" && <CommunitiesPage />}
        {route.view === "community" && <CommunityDetailPage communityId={route.communityId} />}
        {route.view === "backfills" && <BackfillsPage />}
        {route.view === "backfill" && <BackfillDetailPage jobId={route.jobId} />}
        {route.view === "identities" && <IdentitiesPage />}
        {route.view === "identity" && <IdentityDetailPage jobId={route.jobId} />}
        {route.view === "crawl" && <CrawlDetailPage crawlId={route.crawlId} />}
        {route.view === "system" && (
          <SystemView
            storage={storage.data}
            system={system.data}
            history={history.data || []}
          />
        )}
        {route.view === "account" && <AccountPage did={route.did} />}
        {route.view === "record" && (
          <RecordPage
            uri={route.uri}
            onRecord={openRecord}
            onAccount={openAccount}
          />
        )}
        {route.view === "cluster" && (
          <ClusterPage
            clusterId={route.clusterId}
            onAccount={openAccount}
            onRecord={openRecord}
          />
        )}
        {route.view === "trend" && (
          <TrendPage dimension={route.dimension} trendKey={route.trendKey} />
        )}
        </Suspense>
      </main>
    </>
  );
}
