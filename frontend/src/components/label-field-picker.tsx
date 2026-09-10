"use client";

import { useId, useState } from "react";
import { apiFetch } from "@/lib/api";

export interface LabelTerm {
  uri: string;
  local_name: string;
}

export function LabelFieldPicker({ excluded, onSelect }: {
  excluded: string[];
  onSelect: (term: LabelTerm) => void;
}) {
  const [terms, setTerms] = useState<LabelTerm[]>([]);
  const [status, setStatus] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const listId = useId();

  async function load() {
    if (status === "loading" || status === "ready") return;
    setStatus("loading");
    try {
      const fetched = await apiFetch<LabelTerm[]>("/vocabularies/darwin-core");
      // Same vocabulary source and app labels as the occurrence registration form.
      const aliases: Record<string, string> = {
        "http://rs.tdwg.org/dwc/iri/toTaxon": "分類",
        "http://rs.tdwg.org/dwc/terms/decimalLatitude": "緯度",
        "http://rs.tdwg.org/dwc/terms/decimalLongitude": "経度",
      };
      const unique = new Map(fetched.map((term) => [term.uri, {
        ...term, local_name: aliases[term.uri] ?? term.local_name,
      }]));
      for (const [uri, local_name] of Object.entries(aliases)) unique.set(uri, { uri, local_name });
      setTerms([...unique.values()].sort((a, b) => a.local_name.localeCompare(b.local_name, "ja", { sensitivity: "base" })));
      setStatus("ready");
    } catch {
      setStatus("error");
    }
  }

  const normalized = query.trim().toLocaleLowerCase();
  const matches = terms.filter((term) => !excluded.includes(term.uri) &&
    (term.local_name.toLocaleLowerCase().includes(normalized) || term.uri.toLocaleLowerCase().includes(normalized)));
  // Custom absolute URIs remain usable even when the vocabulary service is unavailable.
  const custom = /^[a-z][a-z0-9+.-]*:\S+$/iu.test(query.trim()) && !excluded.includes(query.trim())
    && !matches.some((term) => term.uri === query.trim())
    ? { uri: query.trim(), local_name: query.trim() } : null;
  const options = custom ? [custom, ...matches] : matches;

  function select(term: LabelTerm) {
    onSelect(term);
    setQuery("");
    setOpen(false);
    setActive(0);
  }

  return (
    <div className="mt-4">
      <label className="text-xs text-[#526168]" htmlFor={listId + "-input"}>任意項目を追加</label>
      <input id={listId + "-input"} role="combobox" aria-expanded={open}
        aria-controls={listId} aria-autocomplete="list"
        aria-activedescendant={open && options[active] ? listId + "-" + active : undefined}
        className="mt-1 h-9 w-full rounded border border-[#b8c3c8] px-2 text-sm"
        placeholder="項目名・URI" value={query}
        onFocus={() => { setOpen(true); void load(); }}
        onChange={(event) => { setQuery(event.target.value); setActive(0); setOpen(true); }}
        onBlur={() => setOpen(false)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
            const next = Math.max(0, Math.min(options.length - 1, active + (event.key === "ArrowDown" ? 1 : -1)));
            setActive(next);
            document.getElementById(listId + "-" + next)?.scrollIntoView({ block: "nearest" });
          } else if (event.key === "Enter" && open && options[active]) {
            event.preventDefault();
            select(options[active]);
          } else if (event.key === "Escape" && open) {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
          }
        }} />
      {open && <div className="mt-1 max-h-48 overflow-auto rounded border border-[#b8c3c8] bg-white">
        {status === "loading" && <p role="status" className="p-2 text-xs">読み込み中…</p>}
        {status === "error" && <p role="status" className="p-2 text-xs text-[#a53d32]">候補を取得できませんでした</p>}
        {status === "ready" && options.length === 0 && <p className="p-2 text-xs">一致する項目はありません</p>}
        <div role="listbox" id={listId} aria-label="項目候補">
          {options.map((term, index) => (
            <button type="button" role="option" aria-selected={index === active}
              id={listId + "-" + index} key={term.uri} tabIndex={-1}
              title={term.uri} className={`block w-full break-words px-2 py-2 text-left text-sm hover:bg-[#e8f2ef] ${index === active ? "bg-[#eef2f3]" : ""}`}
              onMouseDown={(event) => event.preventDefault()} onClick={() => select(term)}>
              {term.local_name}
            </button>
          ))}
        </div>
      </div>}
    </div>
  );
}
