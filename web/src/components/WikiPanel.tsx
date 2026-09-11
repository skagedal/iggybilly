import { useState } from "react";

import { ApiError, api } from "../api";
import type { WikiPage } from "../types";

/**
 * A label's Markdown wiki page, with an inline editor.
 *
 * `contentHtml` comes from comrak with `unsafe_ = false`, so raw HTML in
 * the source is escaped server-side — injecting it here is safe, and is
 * the only way to render Markdown we've already turned into HTML.
 */
export function WikiPanel({ page: initial }: { page: WikiPage }) {
  const [page, setPage] = useState(initial);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(initial.content);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startEditing = () => {
    setDraft(page.content);
    setError(null);
    setEditing(true);
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      setPage(await api.saveWiki(page.labelId, draft));
      setEditing(false);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not save the page.");
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="label-wiki">
        <div className="wiki-head">
          <h3 className="wiki-title">{page.labelName}</h3>
        </div>
        <form
          className="wiki-edit"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <textarea
            rows={12}
            autoFocus
            value={draft}
            disabled={saving}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Write this label's wiki page in Markdown… Link to another label with [[label-name]]."
          />
          <p className="hint">
            Markdown supported. Link to another label with{" "}
            <code>[[label-name]]</code> (or <code>[[label-name|text]]</code>).
          </p>
          <div className="wiki-edit-actions">
            <button type="submit" disabled={saving}>
              Save
            </button>
            <button
              type="button"
              className="link"
              disabled={saving}
              onClick={() => setEditing(false)}
            >
              Cancel
            </button>
            {error !== null && <span className="error">{error}</span>}
          </div>
        </form>
      </div>
    );
  }

  return (
    <div className="label-wiki">
      <div className="wiki-head">
        <h3 className="wiki-title">{page.labelName}</h3>
        <span className="wiki-actions">
          <button type="button" className="link" onClick={startEditing}>
            Edit wiki
          </button>
          <a href={`/labels/${page.labelId}/wiki/history`}>History</a>
        </span>
      </div>
      {page.hasContent ? (
        <>
          <div
            className="wiki-content"
            dangerouslySetInnerHTML={{ __html: page.contentHtml }}
          />
          {page.lastEdited !== null && (
            <p className="wiki-meta">{page.lastEdited}</p>
          )}
        </>
      ) : (
        <p className="wiki-empty">
          No wiki page yet.{" "}
          <button type="button" className="link" onClick={startEditing}>
            Write one
          </button>
        </p>
      )}
    </div>
  );
}
