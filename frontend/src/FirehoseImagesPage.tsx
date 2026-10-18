import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight, ExternalLink, Pause, Play, Wifi, WifiOff, X } from "lucide-react";
import { api } from "./api";
import { useIdentityMap } from "./identities";
import { FIREHOSE_PAUSE_EVENT, type LiveConnection } from "./live";
import type { FirehoseEvent, FirehoseFeed, RecordMedia } from "./types";

const BATCH_SIZE = 25;
const MAX_RENDERED = 200;
const MAX_GALLERY_EVENTS = 500;
const EMPTY_EVENTS: FirehoseEvent[] = [];
type MediaBatch = { items: Array<{ uri: string; media: RecordMedia[]; found: boolean }>; source: string; ephemeral: true };
type SafetyMode = "off" | "hide" | "only" | "blur";
type SafetyResult = { score: number; status: "done" | "unknown" };
type NsfwModel = Awaited<ReturnType<(typeof import("nsfwjs/core"))["load"]>>;
const safetyCache = new Map<string, SafetyResult>();
let modelPromise: Promise<NsfwModel> | null = null;

function loadSafetyModel() {
  modelPromise ||= Promise.all([
    import("nsfwjs/core"),
    import("nsfwjs/models/mobilenet_v2"),
  ]).then(([core, definition]) =>
    core.load("MobileNetV2", { modelDefinitions: [definition.MobileNetV2Model] }),
  );
  return modelPromise;
}

function loadClassifiableImage(url: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Image could not be read for local classification"));
    image.src = url;
  });
}

function imageRecordUri(event: FirehoseEvent) {
  return ["post", "reply", "quote"].includes(event.kind) &&
    event.at_uri?.includes("/app.bsky.feed.post/")
    ? event.at_uri
    : undefined;
}

function isImageEvent(event: FirehoseEvent) {
  return Boolean(imageRecordUri(event)) && event.media_kind === "images" && (event.media_count ?? 0) > 0;
}

async function fetchMediaBatch(uris: string[]) {
  const response = await fetch("/api/records/media/batch", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ uris }),
  });
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return response.json() as Promise<MediaBatch>;
}

function Author({ event, handle }: { event: FirehoseEvent; handle?: string }) {
  return handle ? (
    <><b className="block truncate">@{handle}</b><span className="block break-all font-mono text-[9px] leading-tight text-white/35">{event.actor_did}</span></>
  ) : (
    <b className="block break-all font-mono text-[10px] leading-tight">{event.actor_did}</b>
  );
}

type ResolvedImage = { event: FirehoseEvent; image: RecordMedia; handle?: string };

function ImageCard({ item, onOpen, concealed, reveal }: { item?: ResolvedImage; onOpen: () => void; concealed?: boolean; reveal: () => void }) {
  if (!item) return <div className="aspect-square animate-pulse rounded-xl border border-white/[.07] bg-white/[.025]" />;
  return (
    <button type="button" onClick={concealed ? reveal : onOpen} className="group relative block min-w-0 overflow-hidden rounded-xl border border-white/10 bg-white/[.025] text-left transition hover:border-sky-400/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">
      <div className="aspect-square overflow-hidden bg-white/[.02]">
        <img src={item.image.thumbnail_url || item.image.url} alt={item.image.alt || item.event.text || "Bluesky post image"} loading="lazy" referrerPolicy="no-referrer" className={`h-full w-full object-cover transition duration-300 group-hover:scale-[1.02] ${concealed ? "scale-110 blur-2xl" : ""}`} />
      </div>
      {concealed && <span className="absolute inset-x-3 top-1/3 rounded-lg bg-black/80 px-3 py-2 text-center text-xs font-semibold text-white">Potentially sensitive · Reveal</span>}
      <div className="space-y-1.5 p-3">
        <div className="min-w-0 text-xs text-sky-300"><Author event={item.event} handle={item.handle} /></div>
        {item.event.text && <p className="line-clamp-2 text-xs leading-relaxed text-white/70">{item.event.text}</p>}
        <time className="block text-[10px] text-white/35" dateTime={item.event.timestamp}>{new Date(item.event.time_us / 1_000).toLocaleString()}</time>
      </div>
    </button>
  );
}

function Lightbox({ items, index, setIndex, close, onRecord, concealed, reveal }: { items: ResolvedImage[]; index: number; setIndex: (index: number) => void; close: () => void; onRecord: (uri: string) => void; concealed: boolean; reveal: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  const item = items[index];
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const oldOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    dialog.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
      if (event.key === "ArrowLeft" && index > 0) setIndex(index - 1);
      if (event.key === "ArrowRight" && index + 1 < items.length) setIndex(index + 1);
    };
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("keydown", onKey); document.body.style.overflow = oldOverflow; previous?.focus(); };
  }, [close, index, items.length, setIndex]);
  if (!item) return null;
  const handleKey = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") close();
    if (event.key === "ArrowLeft" && index > 0) setIndex(index - 1);
    if (event.key === "ArrowRight" && index + 1 < items.length) setIndex(index + 1);
    if (event.key === "Tab") {
      const controls = [...(dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled)") || [])];
      const first = controls[0];
      const last = controls.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    }
  };
  return (
    <div className="fixed inset-0 z-50 flex bg-black/90 p-2 backdrop-blur-sm sm:p-5" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) close(); }}>
      <div ref={dialog} role="dialog" aria-modal="true" aria-label="Firehose image preview" tabIndex={-1} onKeyDown={handleKey} className="m-auto flex h-full max-h-[calc(100vh-1rem)] w-full max-w-7xl flex-col overflow-hidden rounded-xl border border-white/15 bg-black outline-none sm:max-h-[calc(100vh-2.5rem)] lg:flex-row">
        <div className="relative flex min-h-0 flex-1 items-center justify-center bg-black">
          <img src={item.image.url} alt={item.image.alt || item.event.text || "Bluesky post image"} referrerPolicy="no-referrer" className={`max-h-full max-w-full object-contain ${concealed ? "scale-105 blur-3xl" : ""}`} />
          {concealed && <button type="button" onClick={reveal} className="absolute rounded-lg bg-black/85 px-5 py-3 text-sm font-semibold text-white hover:text-sky-300">Potentially sensitive · Reveal image</button>}
          <button type="button" aria-label="Previous image" disabled={index === 0} onClick={() => setIndex(index - 1)} className="absolute left-2 rounded-full bg-black/75 p-2 text-white disabled:invisible"><ChevronLeft /></button>
          <button type="button" aria-label="Next image" disabled={index + 1 >= items.length} onClick={() => setIndex(index + 1)} className="absolute right-2 rounded-full bg-black/75 p-2 text-white disabled:invisible"><ChevronRight /></button>
          <button type="button" aria-label="Close image preview" onClick={close} className="absolute right-2 top-2 rounded-full bg-black/75 p-2 text-white hover:text-sky-300"><X /></button>
        </div>
        <aside className="max-h-56 shrink-0 overflow-y-auto border-t border-white/10 p-4 lg:max-h-none lg:w-80 lg:border-l lg:border-t-0">
          <div className="text-sm text-sky-300"><Author event={item.event} handle={item.handle} /></div>
          {item.event.text && <p className="mt-4 whitespace-pre-wrap text-sm leading-relaxed text-white/75">{item.event.text}</p>}
          {item.image.alt && <p className="mt-3 text-xs text-white/45">Alt: {item.image.alt}</p>}
          <time className="mt-3 block text-xs text-white/40" dateTime={item.event.timestamp}>{new Date(item.event.time_us / 1_000).toLocaleString()}</time>
          <button type="button" onClick={() => { close(); onRecord(item.event.at_uri); }} className="mt-5 inline-flex items-center gap-2 rounded-lg bg-sky-400 px-4 py-2 text-sm font-semibold text-black hover:bg-sky-300">Open post <ExternalLink className="size-4" /></button>
          <p className="mt-4 text-[10px] text-white/30">{index + 1} of {items.length} · Use arrow keys to navigate</p>
        </aside>
      </div>
    </div>
  );
}

export function FirehoseImagesPage({ onRecord }: { onRecord: (uri: string) => void }) {
  const [paused, setPaused] = useState(false);
  const [gallery, setGallery] = useState<FirehoseEvent[]>([]);
  const [visibleCount, setVisibleCount] = useState(BATCH_SIZE);
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);
  const [safetyMode, setSafetyMode] = useState<SafetyMode>("off");
  const [safetyThreshold, setSafetyThreshold] = useState(0.65);
  const [safetyResults, setSafetyResults] = useState(() => new Map(safetyCache));
  const [modelState, setModelState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [revealed, setRevealed] = useState(() => new Set<string>());
  const closeLightbox = useCallback(() => setLightboxIndex(null), []);
  const loadSentinel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: paused }));
    return () => { if (paused) window.dispatchEvent(new CustomEvent(FIREHOSE_PAUSE_EVENT, { detail: false })); };
  }, [paused]);
  const connection = useQuery<LiveConnection>({ queryKey: ["live-connection"], queryFn: async () => ({ state: "connecting" }), enabled: false });
  const feed = useQuery({ queryKey: ["firehose-feed"], queryFn: () => api<FirehoseFeed>("/api/firehose?limit=500"), refetchInterval: paused || connection.data?.state === "connected" ? false : 3_000 });
  useEffect(() => {
    const incoming = (feed.data?.events || EMPTY_EVENTS).filter(isImageEvent);
    if (!incoming.length) return;
    setGallery((current) => {
      if (!current.length) return [...incoming].reverse().slice(0, MAX_GALLERY_EVENTS);
      const known = new Set(current.map((event) => event.event_id));
      const newestTime = Math.max(...current.map((event) => event.time_us));
      const additions = incoming.filter((event) => event.time_us > newestTime && !known.has(event.event_id));
      return additions.length ? [...current, ...additions].slice(-MAX_GALLERY_EVENTS) : current;
    });
  }, [feed.data?.events]);
  const rendered = gallery.slice(0, Math.min(visibleCount, MAX_RENDERED));
  const handles = useIdentityMap(rendered.map((event) => event.actor_did));
  const chunks = useMemo(() => {
    const result: FirehoseEvent[][] = [];
    for (let index = 0; index < rendered.length; index += BATCH_SIZE) result.push(rendered.slice(index, index + BATCH_SIZE));
    return result;
  }, [rendered]);
  const mediaQueries = useQueries({ queries: chunks.map((chunk) => {
    const uris = chunk.map((event) => event.at_uri);
    return { queryKey: ["record-media-batch", uris], queryFn: () => fetchMediaBatch(uris), staleTime: 300_000, gcTime: 900_000, retry: 0 };
  }) });
  const mediaByUri = useMemo(() => new Map(mediaQueries.flatMap((query) => query.data?.items || []).map((item) => [item.uri, item.media.find((asset) => asset.kind === "image")])), [mediaQueries]);
  const resolved = useMemo(() => rendered.flatMap((event) => {
    const image = mediaByUri.get(event.at_uri);
    return image ? [{ event, image, handle: handles.get(event.actor_did) }] : [];
  }), [handles, mediaByUri, rendered]);
  useEffect(() => {
    if (safetyMode === "off") return;
    let cancelled = false;
    setModelState("loading");
    void loadSafetyModel().then(async (model) => {
      if (!cancelled) setModelState("ready");
      for (const item of resolved) {
        const url = item.image.thumbnail_url || item.image.url;
        if (safetyCache.has(url) || cancelled) continue;
        await new Promise<void>((resolveFrame) => requestAnimationFrame(() => resolveFrame()));
        try {
          const image = await loadClassifiableImage(url);
          const predictions = await model.classify(image);
          const score = predictions.filter((prediction) => ["Porn", "Hentai", "Sexy"].includes(prediction.className)).reduce((total, prediction) => total + prediction.probability, 0);
          safetyCache.set(url, { score, status: "done" });
        } catch {
          safetyCache.set(url, { score: 0, status: "unknown" });
        }
        if (!cancelled) setSafetyResults(new Map(safetyCache));
      }
    }).catch(() => { if (!cancelled) setModelState("error"); });
    return () => { cancelled = true; };
  }, [resolved, safetyMode]);
  const isFlagged = useCallback((item: ResolvedImage) => {
    const result = safetyResults.get(item.image.thumbnail_url || item.image.url);
    return result?.status === "done" && result.score >= safetyThreshold;
  }, [safetyResults, safetyThreshold]);
  const filteredResolved = useMemo(() => resolved.filter((item) => {
    if (safetyMode === "hide") return !isFlagged(item);
    if (safetyMode === "only") return isFlagged(item);
    return true;
  }), [isFlagged, resolved, safetyMode]);
  const flaggedCount = resolved.filter(isFlagged).length;
  const classifiedCount = resolved.filter((item) => safetyResults.get(item.image.thumbnail_url || item.image.url)?.status === "done").length;
  const unknownCount = resolved.filter((item) => safetyResults.get(item.image.thumbnail_url || item.image.url)?.status === "unknown").length;
  useEffect(() => {
    const node = loadSentinel.current;
    if (!node) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) setVisibleCount((count) => Math.min(count + BATCH_SIZE, gallery.length, MAX_RENDERED));
    }, { rootMargin: "500px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [gallery.length, visibleCount]);
  const live = connection.data?.state === "connected";
  const togglePaused = () => setPaused((value) => !value);
  return (
    <div className="space-y-5">
      <div className="flex w-fit rounded-lg border border-white/10 bg-white/[.02] p-1 text-xs"><Link to="/firehose" className="rounded-md px-3 py-1.5 text-white/55 hover:text-sky-300">Events</Link><Link to="/firehose/images" className="rounded-md bg-sky-400 px-3 py-1.5 font-semibold text-black">Images</Link></div>
      <section className="panel p-4 sm:p-5">
        <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div><div className="flex items-center gap-2"><h1 className="font-semibold">Live firehose images</h1><span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold uppercase ${paused ? "bg-amber-400/10 text-amber-300" : live ? "bg-sky-400/10 text-sky-300" : "bg-red-400/10 text-red-300"}`}>{live ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}{paused ? "Paused" : live ? "Streaming" : "Connecting"}</span></div><p className="mt-1 text-xs text-white/45">Public post images are fetched ephemerally in batches and are never retained by the observatory.</p></div>
          <button type="button" onClick={togglePaused} className="inline-flex w-fit items-center gap-1.5 rounded-lg border border-white/10 px-3 py-2 text-xs font-medium text-white/70 hover:border-sky-400/40 hover:text-sky-300">{paused ? <Play className="size-3" /> : <Pause className="size-3" />}{paused ? "Resume" : "Pause"}</button>
        </div>
        <div className="mb-5 flex flex-wrap items-center gap-3 rounded-lg border border-white/10 bg-white/[.02] p-3 text-xs">
          <label className="flex items-center gap-2 text-white/55">Local safety filter
            <select value={safetyMode} onChange={(event) => setSafetyMode(event.target.value as SafetyMode)} className="rounded-md border border-white/10 bg-black px-2 py-1.5 text-white">
              <option value="off">Off</option><option value="hide">Hide flagged</option><option value="only">Only flagged</option><option value="blur">Blur flagged</option>
            </select>
          </label>
          {safetyMode !== "off" && <>
            <label className="flex items-center gap-2 text-white/55">Threshold <input aria-label="Safety threshold" type="range" min="0.4" max="0.95" step="0.05" value={safetyThreshold} onChange={(event) => setSafetyThreshold(Number(event.target.value))} className="accent-sky-400" /><span className="font-mono text-white/75">{Math.round(safetyThreshold * 100)}%</span></label>
            <span className="text-white/40">{modelState === "loading" ? "Loading/scanning locally…" : modelState === "error" ? "Model unavailable — images remain unfiltered" : `${flaggedCount} flagged · ${classifiedCount} scanned · ${unknownCount} unknown`}</span>
          </>}
          <span className="basis-full text-[10px] text-white/30 sm:basis-auto">Opt-in classification runs only in this browser. Images and scores are never sent to or stored by the observatory.</span>
        </div>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
          {rendered.map((event) => {
            const image = mediaByUri.get(event.at_uri);
            const item = image ? { event, image, handle: handles.get(event.actor_did) } : undefined;
            const index = item ? filteredResolved.findIndex((candidate) => candidate.event.event_id === event.event_id) : -1;
            if (item && index < 0) return null;
            if (!item && safetyMode === "only") return null;
            const url = image?.thumbnail_url || image?.url || "";
            const concealed = Boolean(item && safetyMode === "blur" && isFlagged(item) && !revealed.has(url));
            return <ImageCard key={event.event_id} item={item} concealed={concealed} reveal={() => setRevealed((current) => new Set(current).add(url))} onOpen={() => { if (index >= 0) setLightboxIndex(index); }} />;
          })}
        </div>
        {!feed.isLoading && gallery.length === 0 && <div className="grid min-h-80 place-items-center text-sm text-white/45">Waiting for image-bearing posts…</div>}
        <div ref={loadSentinel} className="mt-6 grid min-h-16 place-items-center border-t border-white/10 pt-4 text-xs text-white/45">
          {visibleCount < Math.min(gallery.length, MAX_RENDERED) ? "Scroll to load more images" : visibleCount >= MAX_RENDERED && gallery.length > MAX_RENDERED ? `Showing the newest ${MAX_RENDERED} images to keep this page fast` : `${gallery.length.toLocaleString()} images collected in this live browser buffer`}
        </div>
      </section>
      {lightboxIndex !== null && (() => {
        const item = filteredResolved[lightboxIndex];
        const url = item?.image.thumbnail_url || item?.image.url || "";
        const concealed = Boolean(item && safetyMode === "blur" && isFlagged(item) && !revealed.has(url));
        return <Lightbox items={filteredResolved} index={lightboxIndex} setIndex={setLightboxIndex} close={closeLightbox} onRecord={onRecord} concealed={concealed} reveal={() => setRevealed((current) => new Set(current).add(url))} />;
      })()}
    </div>
  );
}
