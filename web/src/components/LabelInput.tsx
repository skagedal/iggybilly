import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { ApiError, api } from "../api";
import type { ClipLabel, LabelSearchResult } from "../types";

interface Props {
  clipId: number;
  /** Called with the clip's full label list after a successful add. */
  onAdded: (labels: ClipLabel[]) => void;
}

/**
 * The "add a label" field: an autocomplete over existing labels that
 * also offers to create the typed one when it's a valid, unused name.
 */
export function LabelInput({ clipId, onAdded }: Props) {
  const [value, setValue] = useState("");
  const [result, setResult] = useState<LabelSearchResult | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Debounced so a fast typist makes one request rather than one per
  // keystroke. `live` drops a response that a newer query has overtaken.
  useEffect(() => {
    if (!open) return;
    let live = true;
    const timer = window.setTimeout(() => {
      api
        .searchLabels(value, clipId)
        .then((r) => {
          if (live) setResult(r);
        })
        .catch(() => {
          if (live) setResult(null);
        });
    }, 150);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [value, open, clipId]);

  const submit = async (name: string) => {
    const trimmed = name.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    setError(null);
    try {
      onAdded(await api.addLabel(clipId, trimmed));
      setValue("");
      setResult(null);
      setOpen(false);
      inputRef.current?.blur();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not add the label.");
    } finally {
      setBusy(false);
    }
  };

  const suggestions = open && result ? result : null;
  const showList =
    suggestions !== null &&
    (suggestions.matches.length > 0 || suggestions.canCreate || value !== "");

  return (
    <form
      className="label-add"
      onSubmit={(e) => {
        e.preventDefault();
        void submit(value);
      }}
      onBlur={(e) => {
        // Only close when focus leaves the whole widget, so tabbing from
        // the input to a suggestion keeps the list up.
        if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
      }}
    >
      <label>
        Add a label
        <input
          ref={inputRef}
          name="name"
          autoComplete="off"
          value={value}
          disabled={busy}
          onFocus={() => setOpen(true)}
          onChange={(e) => {
            setValue(e.target.value);
            setOpen(true);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") setOpen(false);
          }}
        />
      </label>

      {showList && suggestions && (
        <ul className="suggestions">
          {suggestions.matches.map((name) => (
            <li key={name}>
              <SuggestionButton onPick={() => void submit(name)}>
                {name}
              </SuggestionButton>
            </li>
          ))}
          {suggestions.canCreate && (
            <li className="create">
              <SuggestionButton
                onPick={() => void submit(suggestions.query)}
              >{`Create new label “${suggestions.query}”`}</SuggestionButton>
            </li>
          )}
          {suggestions.matches.length === 0 && !suggestions.canCreate && (
            <li className="empty">No matching labels.</li>
          )}
        </ul>
      )}

      <button type="submit" disabled={busy}>
        Add
      </button>
      {error !== null && <p className="error">{error}</p>}
    </form>
  );
}

/**
 * Suppressing mousedown keeps focus on the input, so the list is still
 * mounted by the time the click lands. Safari in particular doesn't
 * focus a clicked button, which would otherwise blur-and-close first.
 */
function SuggestionButton({
  onPick,
  children,
}: {
  onPick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
    >
      {children}
    </button>
  );
}
