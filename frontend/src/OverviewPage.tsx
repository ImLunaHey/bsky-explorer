import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Chart } from "@tanstack/charts/react";
import { defineChart, lineY } from "@tanstack/charts";
import { scaleLinear } from "@tanstack/charts/scales/linear";
import { tooltip } from "@tanstack/charts/tooltip";
import { ArrowUpRight, RefreshCw } from "lucide-react";
import { api } from "./api";
import { IdentityLink, useIdentityMap } from "./identities";

type Activity = {
  bucket: number;
  events: number;
  posts: number;
  actors: number;
  replies: number;
};
type Totals = Omit<Activity, "bucket"> & {
  current: number;
  first_event: number;
  last_event: number;
};
type Breakdown = { key: string; count: number };
type Rising = {
  uri: string;
  current: number;
  previous: number;
  growth: number;
  participants: number;
  text: string;
  actor: string;
};
type Overview = {
  start: number;
  end: number;
  bucket_seconds: number;
  totals: Totals[];
  activity: Activity[];
  languages: Breakdown[];
  domains: Breakdown[];
  posts: Rising[];
  conversations: Rising[];
};
const number = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});
const periods = [
  { value: "1h", label: "Last hour" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
] as const;
function languageName(code: string) {
  if (code === "und") return "Unspecified";
  try {
    return (
      new Intl.DisplayNames(undefined, { type: "language" }).of(code) || code
    );
  } catch {
    return code;
  }
}
const date = (seconds: number) =>
  new Date(seconds * 1000).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
function change(current: number, previous: number) {
  if (!previous) return current ? "No activity in prior period" : "No change";
  const percent = ((current - previous) / previous) * 100;
  return `${percent > 0 ? "+" : ""}${percent.toFixed(1)}% vs previous period`;
}

function History({ data }: { data: Overview }) {
  const [metric, setMetric] = useState<"posts" | "actors" | "replies">("posts");
  const rows = useMemo(
    () =>
      Array.from({ length: 60 }, (_, index) => {
        const bucket = data.start + index * data.bucket_seconds;
        const row = data.activity.find((item) => item.bucket === bucket);
        return { bucket: bucket * 1000, value: row?.[metric] ?? 0 };
      }),
    [data, metric],
  );
  const definition = useMemo(
    () =>
      defineChart({
        marks: [lineY(rows, { x: "bucket", y: "value", stroke: "#ff8a1f" })],
        scales: {
          x: {
            scale: scaleLinear,
            axis: {
              ticks: {
                format: (v: number) =>
                  new Date(v).toLocaleString([], {
                    month: "short",
                    day: "numeric",
                    hour: "2-digit",
                    minute: "2-digit",
                  }),
              },
            },
          },
          y: {
            scale: scaleLinear,
            axis: { ticks: { format: (v: number) => number.format(v) } },
          },
        },
        focus: "nearest-x",
        tooltip: {
          use: tooltip,
          items: [
            {
              channel: "x",
              label: "Time",
              text: (item) => new Date(Number(item.xValue)).toLocaleString(),
            },
            {
              channel: "y",
              label: metric,
              text: (item) => Number(item.yValue).toLocaleString(),
            },
          ],
        },
      }),
    [rows, metric],
  );
  return (
    <section className="panel p-5 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="font-semibold">The rhythm of the network</h2>
          <p className="mt-1 text-xs text-dim">
            {Math.round(data.bucket_seconds / 60)}-minute buckets · missing
            activity is not proof the network was quiet
          </p>
        </div>
        <div className="flex gap-1">
          {(["posts", "actors", "replies"] as const).map((value) => (
            <button
              key={value}
              aria-pressed={metric === value}
              onClick={() => setMetric(value)}
              className={`rounded-lg px-3 py-2 text-xs capitalize ${metric === value ? "bg-orange-500/15 text-orange-300" : "text-dim"}`}
            >
              {value === "actors" ? "Active people" : value}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-6">
        <Chart
          definition={definition}
          height={240}
          ariaLabel={`${metric} over the selected period`}
        />
      </div>
    </section>
  );
}

function RisingList({
  title,
  note,
  rows,
}: {
  title: string;
  note: string;
  rows: Rising[];
}) {
  const identities = useIdentityMap(rows.map((row) => row.actor));
  return (
    <section className="panel overflow-hidden">
      <header className="border-b border-line p-5">
        <h2 className="font-semibold">{title}</h2>
        <p className="mt-1 text-xs leading-relaxed text-dim">{note}</p>
      </header>
      <div className="divide-y divide-line">
        {rows.length ? (
          rows.map((row, index) => (
            <article key={row.uri} className="p-5">
              <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 gap-3">
                  <span className="mt-0.5 text-xs text-dim">
                    {String(index + 1).padStart(2, "0")}
                  </span>
                  <IdentityLink
                    did={row.actor}
                    handle={identities.get(row.actor)}
                    compact
                  />
                </div>
                <span className="shrink-0 rounded-full bg-orange-500/10 px-2 py-1 text-xs text-orange-300">
                  {row.growth > 0 ? "+" : ""}
                  {number.format(row.growth)}
                </span>
              </div>
              <Link
                to="/records/$uri"
                params={{ uri: row.uri }}
                className="mt-3 block text-sm leading-relaxed hover:text-orange-200"
              >
                <span className="line-clamp-3">
                  {row.text ||
                    "Open post · its text is outside this comparison window"}
                </span>
              </Link>
              <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-dim">
                <span>{number.format(row.current)} interactions</span>
                <span>{number.format(row.participants)} people</span>
                <span>{number.format(row.previous)} in prior period</span>
              </div>
            </article>
          ))
        ) : (
          <p className="p-8 text-sm text-dim">
            No observed activity in this period.
          </p>
        )}
      </div>
    </section>
  );
}

export function OverviewPage() {
  const [window, setWindow] = useState<(typeof periods)[number]["value"]>(
    () => {
      const value = new URLSearchParams(globalThis.location.search).get(
        "window",
      );
      return value === "24h" || value === "7d" ? value : "1h";
    },
  );
  const query = useQuery({
    queryKey: ["overview", window],
    queryFn: () => api<Overview>(`/api/overview?window=${window}`),
    staleTime: 60_000,
    refetchInterval: 60_000,
    retry: 1,
  });
  const data = query.data;
  const current = data?.totals.find((item) => item.current === 1);
  const previous = data?.totals.find((item) => item.current === 0);
  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-end justify-between gap-5">
        <div>
          <p className="eyebrow">A window into the public network</p>
          <h1 className="mt-2 text-3xl font-semibold tracking-tight sm:text-4xl">
            Bluesky now<span className="text-orange-400">.</span>
          </h1>
          <p className="mt-3 max-w-2xl text-sm leading-relaxed text-dim">
            Who’s here, what’s gathering attention, and how the conversation is
            changing.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div
            className="flex rounded-xl border border-line bg-panel p-1"
            role="group"
            aria-label="Dashboard time range"
          >
            {periods.map((period) => (
              <button
                key={period.value}
                aria-pressed={window === period.value}
                onClick={() => {
                  setWindow(period.value);
                  const url = new URL(globalThis.location.href);
                  url.searchParams.set("window", period.value);
                  globalThis.history.replaceState(null, "", url);
                }}
                className={`rounded-lg px-3 py-2 text-xs ${window === period.value ? "bg-mint font-medium text-ink" : "text-dim hover:text-white"}`}
              >
                {period.label}
              </button>
            ))}
          </div>
          <button
            onClick={() => void query.refetch()}
            disabled={query.isFetching}
            aria-label="Refresh overview"
            className="rounded-lg border border-line p-2.5 disabled:opacity-40"
          >
            <RefreshCw
              className={`size-4 ${query.isFetching ? "animate-spin" : ""}`}
            />
          </button>
        </div>
      </header>
      {query.isError && (
        <div
          role="alert"
          className="rounded-xl border border-red-500/30 bg-red-500/5 p-5 text-sm text-red-200"
        >
          Couldn’t load this period. Try refreshing or choosing a shorter range.
          {data && " Showing the last successful result."}
        </div>
      )}
      {!data && query.isPending && (
        <div
          role="status"
          className="panel grid min-h-80 place-items-center p-8 text-dim"
        >
          Reading retained network activity… Day and week ranges can take a few
          minutes on first load.
        </div>
      )}
      {data && (
        <>
          <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-dim">
            <span>
              {date(data.start)} — {date(data.end)} · estimated counts
            </span>
            <Link
              to="/coverage"
              className="inline-flex items-center gap-1 hover:text-orange-300"
            >
              What do these numbers cover? <ArrowUpRight className="size-3" />
            </Link>
          </div>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {(
              [
                {
                  key: "actors",
                  label: "Active people",
                  note: "Unique accounts taking public actions",
                },
                {
                  key: "posts",
                  label: "Posts published",
                  note: "Includes replies and quote posts",
                },
                {
                  key: "replies",
                  label: "Replies",
                  note: "Public conversation activity",
                },
                {
                  key: "events",
                  label: "Public actions",
                  note: "Observed Bluesky record events",
                },
              ] as const
            ).map((stat) => (
              <div key={stat.key} className="panel p-5">
                <p className="eyebrow">{stat.label}</p>
                <p className="mt-3 text-3xl font-semibold tabular-nums">
                  {number.format(current?.[stat.key] ?? 0)}
                </p>
                <p className="mt-2 text-xs text-orange-200">
                  {change(current?.[stat.key] ?? 0, previous?.[stat.key] ?? 0)}
                </p>
                <p className="mt-2 text-xs text-dim">{stat.note}</p>
              </div>
            ))}
          </div>
          <History data={data} />
          <div className="grid gap-5 lg:grid-cols-2">
            <RisingList
              title="Posts gaining attention"
              note="Ranked by the increase in likes, reposts, replies, and quotes over the previous equal-length period."
              rows={data.posts}
            />
            <RisingList
              title="Conversations picking up"
              note="Threads with at least two replying accounts, ranked by growth in replies."
              rows={data.conversations}
            />
          </div>
          <div className="grid gap-5 lg:grid-cols-2">
            <section className="panel p-5">
              <h2 className="font-semibold">Languages on the network</h2>
              <p className="mt-1 text-xs text-dim">
                Declared post languages; multilingual posts appear in more than
                one group.
              </p>
              <div className="mt-5 space-y-3">
                {data.languages.map((item) => (
                  <div key={item.key}>
                    <div className="mb-1 flex justify-between text-xs">
                      <span>{languageName(item.key)}</span>
                      <span className="text-dim">
                        {number.format(item.count)} posts
                      </span>
                    </div>
                    <div className="h-1.5 rounded bg-white/5">
                      <div
                        className="h-full rounded bg-orange-400"
                        style={{
                          width: `${(item.count / Math.max(1, data.languages[0]?.count ?? 1)) * 100}%`,
                        }}
                      />
                    </div>
                  </div>
                ))}
                {!data.languages.length && (
                  <p className="text-sm text-dim">
                    No posts observed in this period.
                  </p>
                )}
              </div>
            </section>
            <section className="panel p-5">
              <h2 className="font-semibold">Links people are sharing</h2>
              <p className="mt-1 text-xs text-dim">
                Domains mentioned in posts during this period.
              </p>
              <div className="mt-4 divide-y divide-line">
                {data.domains.map((item) => (
                  <div
                    key={item.key}
                    className="flex justify-between gap-3 py-3 text-sm"
                  >
                    <span className="truncate">{item.key}</span>
                    <span className="shrink-0 text-dim">
                      {number.format(item.count)} posts
                    </span>
                  </div>
                ))}
                {!data.domains.length && (
                  <p className="py-4 text-sm text-dim">
                    No shared links observed in this period.
                  </p>
                )}
              </div>
            </section>
          </div>
          <Link
            to="/communities"
            className="panel flex items-center justify-between gap-4 p-6 hover:border-orange-500/50"
          >
            <div>
              <p className="eyebrow">Go beyond the global view</p>
              <h2 className="mt-2 text-lg font-semibold">
                Explore communities
              </h2>
              <p className="mt-2 text-sm text-dim">
                Follow the connections between people and discover different
                corners of the network. Community snapshots have their own time
                range.
              </p>
            </div>
            <ArrowUpRight className="size-6 shrink-0 text-orange-300" />
          </Link>
          <aside className="rounded-xl border border-line p-4 text-xs leading-relaxed text-dim">
            Rankings compare the 1,000 most active candidate posts or threads in
            each period. Counts are estimates from retained public Bluesky
            events, not a complete platform census. Distinct-count sketches
            deduplicate event IDs and use event timestamps; backfills can change
            historical results. Empty buckets may reflect collection gaps.{" "}
            {current
              ? `Latest event in this period: ${date(current.last_event)}.`
              : "No events retained in this period."}{" "}
            Open a post for its available record and context; that detail is not
            restricted to this dashboard’s time range.
          </aside>
        </>
      )}
    </div>
  );
}
