import { useState } from "react";

import { ApiError, api } from "../api";
import { LabelInput } from "../components/LabelInput";
import { Layout } from "../components/Layout";
import { Waveform } from "../components/Waveform";
import { formatTime } from "../format";
import { trackFor, usePlayer } from "../player";
import { useRouter } from "../router";
import type { ClipDetail, ClipLabel, ClipProps } from "../types";

export default function ClipPage({ username, clip }: ClipProps) {
  const [name, setName] = useState(clip.name);
  const [labels, setLabels] = useState(clip.labels);

  const nav = (
    <>
      <a href="/">Clips</a>
      <a href="/account">Account</a>
    </>
  );

  return (
    <Layout username={username} nav={nav}>
      <section>
        <ClipHeader clipId={clip.id} name={name} onRenamed={setName} />

        <p className="meta">
          {clip.recordingDate !== null && (
            <>
              Recorded {clip.recordingDate}
              <br />
            </>
          )}
          Uploaded by {clip.uploader} on {clip.uploadedAt}
          <br />
          <span className="orig">{clip.originalFilename}</span> (
          {clip.contentType})
        </p>

        <Player clip={{ ...clip, name }} />

        <h2>Labels</h2>
        <LabelList clipId={clip.id} labels={labels} onChanged={setLabels} />
        <LabelInput clipId={clip.id} onAdded={setLabels} />

        {clip.canDelete && <DeleteClip clipId={clip.id} name={name} />}
      </section>
    </Layout>
  );
}

function ClipHeader({
  clipId,
  name,
  onRenamed,
}: {
  clipId: number;
  name: string;
  onRenamed: (name: string) => void;
}) {
  const player = usePlayer();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const result = await api.renameClip(clipId, draft);
      onRenamed(result.name);
      // If this clip is the one playing, the bar's caption is now stale.
      player.rename(clipId, result.name);
      setEditing(false);
      document.title = `${result.name} — iggybilly`;
    } catch (e) {
      // A 409 means the name is taken; the message is worth showing
      // verbatim, since it names the clash.
      setError(e instanceof ApiError ? e.message : "Could not rename.");
    } finally {
      setSaving(false);
    }
  };

  if (!editing) {
    return (
      <div className="clip-header">
        <h1>{name}</h1>
        <button
          type="button"
          className="rename-btn"
          onClick={() => {
            setDraft(name);
            setError(null);
            setEditing(true);
          }}
        >
          Rename
        </button>
      </div>
    );
  }

  return (
    <form
      className="clip-header rename-form"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <input
        value={draft}
        autoFocus
        required
        disabled={saving}
        onChange={(e) => setDraft(e.target.value)}
      />
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
    </form>
  );
}

/**
 * The detail page's large waveform. Playback itself belongs to the
 * global bar, so this draws the peaks, mirrors the bar's progress when
 * this is the loaded clip, and hands play/seek down to it.
 */
function Player({ clip }: { clip: ClipDetail }) {
  const player = usePlayer();
  const isCurrent = player.track?.clipId === clip.id;
  const isPlaying = isCurrent && player.isPlaying;

  return (
    <>
      <div className="clip-waveform">
        <Waveform
          peaks={clip.peaks}
          progress={isCurrent ? player.progress : 0}
          height={96}
          onSeek={isCurrent ? player.seek : undefined}
        />
      </div>
      <div className="player-controls">
        <button type="button" onClick={() => player.play(trackFor(clip))}>
          {isPlaying ? "Pause" : "Play"}
        </button>
        <span className="clip-time">
          {isCurrent
            ? formatTime(player.currentTime)
            : clip.durationSeconds !== null
              ? formatTime(clip.durationSeconds)
              : "0:00"}
        </span>
        <a
          className="download"
          href={`/clips/${clip.id}/audio?download=1`}
          download={clip.originalFilename}
        >
          Download
        </a>
      </div>
    </>
  );
}

function LabelList({
  clipId,
  labels,
  onChanged,
}: {
  clipId: number;
  labels: ClipLabel[];
  onChanged: (labels: ClipLabel[]) => void;
}) {
  const [busy, setBusy] = useState(false);

  const remove = async (labelId: number) => {
    if (busy) return;
    setBusy(true);
    try {
      onChanged(await api.removeLabel(clipId, labelId));
    } finally {
      setBusy(false);
    }
  };

  if (labels.length === 0) {
    return (
      <ul className="label-list">
        <li className="empty">No labels yet.</li>
      </ul>
    );
  }

  return (
    <ul className="label-list">
      {labels.map((label) => (
        <li className="label" key={label.id}>
          <a href={label.filterHref}>{label.name}</a>
          <button
            className="remove"
            type="button"
            title="Remove label"
            disabled={busy}
            onClick={() => void remove(label.id)}
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  );
}

function DeleteClip({ clipId, name }: { clipId: number; name: string }) {
  const player = usePlayer();
  const { navigate } = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const remove = async () => {
    const ok = window.confirm(
      `Delete “${name}”? This removes the clip and its audio for everyone, and can't be undone.`,
    );
    if (!ok) return;
    setBusy(true);
    setError(null);
    try {
      await api.deleteClip(clipId);
      // Its audio is gone from disk, so stop it before leaving —
      // otherwise the bar would keep a dead URL loaded.
      if (player.track?.clipId === clipId) player.stop();
      navigate("/");
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not delete the clip.");
      setBusy(false);
    }
  };

  return (
    <div className="clip-actions">
      <button
        type="button"
        className="delete-btn"
        disabled={busy}
        onClick={() => void remove()}
      >
        Delete clip
      </button>
      {error !== null && <p className="error">{error}</p>}
    </div>
  );
}
