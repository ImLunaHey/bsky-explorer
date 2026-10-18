import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { Search } from "lucide-react";

const destinations = [
  { label: "ATProto now", hint: "People, posts and conversations", to: "/" },
  { label: "Firehose", hint: "Newest public ATProto events", to: "/firehose" },
  { label: "Firehose images", hint: "Live public image posts", to: "/firehose/images" },
  { label: "Search", hint: "Find accounts, posts, and events", to: "/search" },
  { label: "Coverage", hint: "Retained platform mirror overview", to: "/coverage" },
  { label: "Communities", hint: "Interaction community network", to: "/communities" },
  {
    label: "Backfills",
    hint: "Durable account collection queue",
    to: "/backfills",
  },
  {
    label: "Identities",
    hint: "DID and handle resolution operations",
    to: "/identities",
  },
  {
    label: "Detections",
    hint: "Coordination and anomaly evidence",
    to: "/detections",
  },
  {
    label: "System",
    hint: "Host, ClickHouse, and storage health",
    to: "/system",
  },
] as const;

export function CommandPalette() {
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen((current) => !current);
      }
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
  useEffect(() => {
    if (open) {
      setValue("");
      window.setTimeout(() => input.current?.focus(), 0);
    }
  }, [open]);
  const needle = value.trim().toLowerCase();
  const matches = destinations.filter((item) =>
    `${item.label} ${item.hint}`.toLowerCase().includes(needle),
  );
  const direct = value.trim();
  const isDid = /^did:[a-z0-9]+:[A-Za-z0-9._:%-]+$/.test(direct);
  const isRecord = /^at:\/\/[^/]+\/[^/]+\/[^/]+$/.test(direct);
  const go = () => {
    if (isDid) navigate({ to: "/accounts/$did", params: { did: direct } });
    else if (isRecord)
      navigate({ to: "/records/$uri", params: { uri: direct } });
    else if (matches[0]) navigate({ to: matches[0].to });
    else if (direct)
      navigate({
        to: "/search",
        search: { q: direct, type: "all", offset: 0 },
      });
    else return;
    setOpen(false);
  };
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="Open command palette"
        className="inline-flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs text-dim hover:border-sky-500/50 hover:text-white"
      >
        <Search className="size-3.5" />
        <span className="hidden sm:inline">Search & navigate</span>
        <kbd className="hidden rounded border border-line px-1.5 py-0.5 font-mono text-[9px] sm:inline">
          ⌘K
        </kbd>
      </button>
      {open && (
        <div
          className="fixed inset-0 z-50 flex items-start justify-center bg-black/80 px-4 pt-[12vh] backdrop-blur-sm"
          onMouseDown={(event) => {
            if (event.currentTarget === event.target) setOpen(false);
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label="Navigate the observatory"
            className="w-full max-w-xl overflow-hidden rounded-2xl border border-sky-500/30 bg-black shadow-2xl shadow-sky-950/30"
          >
            <div className="flex items-center gap-3 border-b border-line px-4">
              <Search className="size-5 text-sky-400" />
              <input
                ref={input}
                value={value}
                onChange={(event) => setValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") go();
                }}
                aria-label="Page, DID, AT URI, or search terms"
                placeholder="Page, DID, AT URI, or search terms…"
                className="h-14 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-dim"
              />
              <kbd className="text-[10px] text-dim">Esc</kbd>
            </div>
            <div className="max-h-80 overflow-y-auto p-2">
              {(isDid || isRecord) && (
                <button
                  autoFocus={false}
                  onClick={go}
                  className="flex w-full items-center justify-between rounded-xl px-3 py-3 text-left hover:bg-sky-500/10"
                >
                  <span>
                    <b className="block text-sm text-sky-300">
                      {isDid ? "Open account" : "Open record"}
                    </b>
                    <span className="block max-w-md break-all font-mono text-[10px] text-dim">
                      {direct}
                    </span>
                  </span>
                  <span className="text-xs text-dim">Enter</span>
                </button>
              )}
              {matches.map((item) => (
                <button
                  key={item.to}
                  onClick={() => {
                    navigate({ to: item.to });
                    setOpen(false);
                  }}
                  className="block w-full rounded-xl px-3 py-3 text-left hover:bg-sky-500/10"
                >
                  <b className="block text-sm">{item.label}</b>
                  <span className="text-xs text-dim">{item.hint}</span>
                </button>
              ))}
              {!matches.length && !isDid && !isRecord && direct && (
                <button
                  onClick={go}
                  className="flex w-full items-center justify-between rounded-xl px-3 py-3 text-left hover:bg-sky-500/10"
                >
                  <span>
                    <b className="block text-sm text-sky-300">
                      Search retained data
                    </b>
                    <span className="block max-w-md break-all text-xs text-dim">
                      {direct}
                    </span>
                  </span>
                  <span className="text-xs text-dim">Enter</span>
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
