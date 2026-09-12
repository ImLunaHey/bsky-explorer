import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight, Pause, Play, Wifi, WifiOff } from "lucide-react";
import { api } from "./api";
import { useIdentityMap } from "./identities";
import { FIREHOSE_PAUSE_EVENT, type LiveConnection } from "./live";
import type { FirehoseEvent, FirehoseFeed, RecordMedia } from "./types";

const PAGE_SIZE = 25;
const EMPTY_EVENTS: FirehoseEvent[] = [];

function imageRecordUri(event: FirehoseEvent) {
  return ["post", "reply", "quote"].includes(event.kind) &&
    event.at_uri?.includes("/app.bsky.feed.post/")
    ? event.at_uri
    : undefined;
}

function ImageCard({
  event,
  handle,
  image,
  loaded,
  onRecord,
}: {
  event: FirehoseEvent;
  handle?: string;
  image?: RecordMedia;
  loaded: boolean;
  onRecord: (uri: string) => void;
}) {
  const uri = imageRecordUri(event)!;
  if (loaded && (!image || image.kind !== "image")) return null;
  return (
    <div className="min-w-0">
      {!image ? (
        <div className="aspect-square animate-pulse rounded-xl border border-white/[.07] bg-white/[.025]" />
      ) : (
        <button
          type="button"
          onClick={() => onRecord(uri)}
          className="group block w-full overflow-hidden rounded-xl border border-white/10 bg-white/[.025] text-left transition hover:border-orange-400/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange-400"
        >
          <div className="aspect-square overflow-hidden bg-white/[.02]">
            <img
              src={image.url}
              alt={image.alt || event.text || "Bluesky post image"}
              loading="lazy"
              referrerPolicy="no-referrer"
              className="h-full w-full object-cover transition duration-300 group-hover:scale-[1.02]"
            />
          </div>
          <div className="space-y-1.5 p-3">
            <div className="min-w-0 text-xs text-orange-300">
              {handle ? (
                <><b className="block truncate">@{handle}</b><span className="block break-all font-mono text-[9px] leading-tight text-white/35">{event.actor_did}</span></>
              ) : (
                <b className="block break-all font-mono text-[10px] leading-tight">{event.actor_did}</b>
              )}
            </div>
            {event.text && <p className="line-clamp-2 text-xs leading-relaxed text-white/70">{event.text}</p>}
            <time className="block text-[10px] text-white/35" dateTime={event.timestamp}>
              {new Date(event.time_us / 1_000).toLocaleString()}
            </time>
          </div>
        </button>
      )}
    </div>
  );
}

export function FirehoseImagesPage({ onRecord }: { onRecord: (uri: string) => void }) {
  const [paused, setPaused] = useState(false);
  const [snapshot, setSnapshot] = useState<FirehoseEvent[] | null>(null);
  const [page, setPage] = useState(0);
  const gridHost = useRef<HTMLDivElement>(null);
  const [nearViewport, setNearViewport] = useState(false);
  useEffect(() => {
    window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: paused }));
    return () => {
      if (paused) window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: false }));
    };
  }, [paused]);
  const connection = useQuery<LiveConnection>({
    queryKey: ["live-connection"],
    queryFn: async () => ({ state: "connecting" }),
    enabled: false,
  });
  const feed = useQuery({
    queryKey: ["firehose-feed"],
    queryFn: () => api<FirehoseFeed>("/api/firehose?limit=500"),
    refetchInterval: paused || connection.data?.state === "connected" ? false : 3_000,
  });
  const candidates = useMemo(
    () => [...(feed.data?.events || EMPTY_EVENTS)].reverse().filter((event) => {
      if (!imageRecordUri(event)) return false;
      return !event.media_kind || ["images", "record_with_media", "unknown"].includes(event.media_kind);
    }),
    [feed.data?.events],
  );
  const events = paused ? snapshot || EMPTY_EVENTS : candidates;
  const pageCount = Math.max(1, Math.ceil(events.length / PAGE_SIZE));
  const visible = events.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const handles = useIdentityMap(visible.map((event) => event.actor_did));
  const uris = useMemo(() => visible.map(imageRecordUri).filter((uri): uri is string => Boolean(uri)), [visible]);
  useEffect(() => {
    setNearViewport(false);
    const node = gridHost.current;
    if (!node) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) {
        setNearViewport(true);
        observer.disconnect();
      }
    }, { rootMargin: "320px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [page]);
  const media = useQuery({
    queryKey: ["record-media-batch", uris],
    queryFn: async () => {
      const response = await fetch("/api/records/media/batch", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ uris }),
      });
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      return response.json() as Promise<{ items: Array<{ uri: string; media: RecordMedia[]; found: boolean }>; source: string; ephemeral: true }>;
    },
    enabled: nearViewport && uris.length > 0,
    staleTime: 300_000,
    gcTime: 900_000,
    retry: 0,
  });
  const mediaByUri = useMemo(() => new Map(media.data?.items.map((item) => [item.uri, item.media.find((asset) => asset.kind === "image")]) || []), [media.data?.items]);
  const togglePaused = () => {
    if (paused) {
      setSnapshot(null);
      setPage(0);
      setPaused(false);
    } else {
      setSnapshot(candidates);
      setPaused(true);
    }
  };
  const live = connection.data?.state === "connected";
  return (
    <div className="space-y-5">
      <div className="flex w-fit rounded-lg border border-white/10 bg-white/[.02] p-1 text-xs">
        <Link to="/firehose" className="rounded-md px-3 py-1.5 text-white/55 hover:text-orange-300">Events</Link>
        <Link to="/firehose/images" className="rounded-md bg-orange-400 px-3 py-1.5 font-semibold text-black">Images</Link>
      </div>
      <section className="panel p-4 sm:p-5">
        <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <h1 className="font-semibold">Live firehose images</h1>
              <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase ${paused ? "bg-amber-400/10 text-amber-300" : live ? "bg-orange-400/10 text-orange-300" : "bg-red-400/10 text-red-300"}`}>
                {live ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}
                {paused ? "Paused" : live ? "Streaming" : "Connecting"}
              </span>
            </div>
            <p className="mt-1 text-xs text-white/45">Public post images are fetched ephemerally near the viewport and are never retained by the observatory.</p>
          </div>
          <button type="button" onClick={togglePaused} className="inline-flex w-fit items-center gap-1.5 rounded-lg border border-white/10 px-3 py-2 text-xs font-medium text-white/70 hover:border-orange-400/40 hover:text-orange-300">
            {paused ? <Play className="size-3" /> : <Pause className="size-3" />}{paused ? "Resume" : "Pause"}
          </button>
        </div>
        <div ref={gridHost} className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
          {visible.map((event) => <ImageCard key={event.event_id} event={event} handle={handles.get(event.actor_did)} image={mediaByUri.get(event.at_uri)} loaded={media.isFetched} onRecord={onRecord} />)}
        </div>
        {!feed.isLoading && visible.length === 0 && <div className="grid min-h-80 place-items-center text-sm text-white/45">Waiting for image-bearing posts…</div>}
        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-white/10 pt-4 text-xs text-white/45">
          <span>Page {page + 1} of {pageCount} · {events.length.toLocaleString()} candidate posts in the bounded live cache</span>
          <div className="flex gap-2">
            <button aria-label="Previous image page" disabled={page === 0} onClick={() => setPage((value) => Math.max(0, value - 1))} className="rounded border border-white/10 p-1.5 disabled:opacity-30"><ChevronLeft className="size-4" /></button>
            <button aria-label="Next image page" disabled={page + 1 >= pageCount} onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))} className="rounded border border-white/10 p-1.5 disabled:opacity-30"><ChevronRight className="size-4" /></button>
          </div>
        </div>
      </section>
    </div>
  );
}
