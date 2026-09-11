import { useState } from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";
import { useRouter } from "../router";
import type { WikiHistoryProps, WikiRevision } from "../types";

export default function WikiHistoryPage({
  username,
  labelId,
  labelName,
  revisions,
}: WikiHistoryProps) {
  const nav = (
    <>
      <a href="/">Clips</a>
      <a href="/account">Account</a>
    </>
  );

  return (
    <Layout username={username} nav={nav}>
      <section>
        <h2>Wiki history: {labelName}</h2>
        <p className="wiki-back">
          <a href={`/?label=${encodeURIComponent(labelName)}`}>
            ← back to {labelName} clips
          </a>
        </p>

        {revisions.length === 0 ? (
          <p>This label has no wiki page yet.</p>
        ) : (
          <ol className="wiki-history">
            {revisions.map((revision) => (
              <Revision
                key={revision.id}
                labelId={labelId}
                revision={revision}
              />
            ))}
          </ol>
        )}
      </section>
    </Layout>
  );
}

function Revision({
  labelId,
  revision,
}: {
  labelId: number;
  revision: WikiRevision;
}) {
  const { navigate } = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const restore = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.restoreWikiRevision(labelId, revision.id);
      // Restoring appends a revision, so re-fetch this same page to
      // pick it up — as a client navigation, so playback isn't cut off.
      navigate(window.location.pathname, { replace: true });
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not restore.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="wiki-rev">
      <div className="rev-head">
        <span className="rev-when">{revision.editedAt}</span>
        <span className="rev-author">by {revision.author}</span>
        {revision.isCurrent ? (
          <span className="rev-current">current</span>
        ) : (
          <button
            type="button"
            className="link"
            disabled={busy}
            onClick={() => void restore()}
          >
            Restore this version
          </button>
        )}
        {error !== null && <span className="error">{error}</span>}
      </div>
      <details className="rev-body" open={revision.isCurrent}>
        <summary>View content</summary>
        <div
          className="wiki-content"
          dangerouslySetInnerHTML={{ __html: revision.contentHtml }}
        />
      </details>
    </li>
  );
}
