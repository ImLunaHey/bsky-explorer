/* eslint-disable react-refresh/only-export-components */
import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { api } from "./api";
import type { Identity } from "./types";

export const didFromValue = (value: string) =>
  value.startsWith("at://")
    ? value.slice(5).split("/")[0]
    : value.startsWith("did:")
      ? value
      : undefined;

export function useIdentityMap(values: string[]) {
  const dids = [
    ...new Set(
      values.map(didFromValue).filter((did): did is string => Boolean(did)),
    ),
  ].sort();
  const batches = Array.from(
    { length: Math.ceil(dids.length / 100) },
    (_, index) => dids.slice(index * 100, (index + 1) * 100),
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
  const identities=queries.flatMap((query) => query.data || []);
  const signature=identities.map(identity=>`${identity.did}\0${identity.handle||''}`).join('\0');
  // The signature keeps the returned Map referentially stable when unrelated
  // query notifications rerender a page with unchanged identity data.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(()=>new Map(identities.map(identity=>[identity.did,identity.handle||undefined])),[signature]);
}

export function IdentityLink({
  did,
  handle,
  compact = false,
  className = "",
}: {
  did: string;
  handle?: string;
  compact?: boolean;
  className?: string;
}) {
  return (
    <Link
      to="/accounts/$did"
      params={{ did }}
      title={`${handle ? `@${handle} · ` : ""}${did}`}
      className={`block min-w-0 text-left text-orange-300 hover:text-orange-200 ${className}`}
    >
      <span className="block break-all font-medium">
        {handle ? `@${handle}` : did}
      </span>
      {handle ? (
        <span
          className={`block break-all font-mono text-white/35 ${compact ? "text-[9px]" : "text-[10px]"}`}
        >
          {did}
        </span>
      ) : (
        <span className="block text-[9px] text-dim">resolving…</span>
      )}
    </Link>
  );
}
