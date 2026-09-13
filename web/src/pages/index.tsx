import { useEffect, useRef, useState } from "react";
import type {
  DragEvent as ReactDragEvent,
  KeyboardEvent as ReactKeyboardEvent,
  ReactNode,
} from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";
import { Waveform } from "../components/Waveform";
import { WikiPanel } from "../components/WikiPanel";
import { formatTime } from "../format";
import { trackFor, usePlayer } from "../player";
import { useRouter } from "../router";
import type { ClipSummary, IndexProps, PlaylistInfo } from "../types";

export default function IndexPage({
  username,
  clips,
  activeFilters,
  activeWikis,
  playlist,
}: IndexProps) {
  return (
    <Layout username={username} nav={<a href="/account">Account</a>}>
      <section>
        <h2>Upload</h2>
        <Uploader />
      </section>

      <section>
        <h2>Clips</h2>

        {activeFilters.length > 0 && (
          <div className="active-filters">
            <span className="label-prefix">Filtering by:</span>
            {activeFilters.map((filter) => (
              <span className="filter-chip" key={filter.name}>
                {filter.name}
                <a
                  className="remove"
                  href={filter.removeHref}
                  title="Remove filter"
                >
                  ×
                </a>
              </span>
            ))}
            <a className="clear" href="/">
              Clear all
            </a>
          </div>
        )}

        {activeWikis.length > 0 && (
          <div className="label-wikis">
            {activeWikis.map((wiki) => (
              <WikiPanel key={wiki.labelId} page={wiki} />
            ))}
          </div>
        )}

        {clips.length === 0 ? (
          <p>
            {activeFilters.length === 0
              ? "No clips yet. Upload the first one above."
              : "No clips match these labels."}
          </p>
        ) : playlist !== null ? (
          <Playlist playlist={playlist} clips={clips} />
        ) : (
          <ul className="clip-list">
            {clips.map((clip) => (
              <ClipCard key={clip.id} clip={clip} />
            ))}
          </ul>
        )}
      </section>
    </Layout>
  );
}

/**
 * Multi-file upload, by picker or by drop. On success we navigate to the
 * unfiltered list, since a freshly uploaded clip has no labels yet and
 * would be invisible under an active filter.
 */
function Uploader() {
  const { navigate } = useRouter();
  const [uploading, setUploading] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // A file dropped outside the zone would otherwise make the browser
  // navigate to it and lose the page — swallow those drops.
  useEffect(() => {
    const swallow = (e: DragEvent) => e.preventDefault();
    window.addEventListener("dragover", swallow);
    window.addEventListener("drop", swallow);
    return () => {
      window.removeEventListener("dragover", swallow);
      window.removeEventListener("drop", swallow);
    };
  }, []);

  const upload = async (files: FileList | null) => {
    if (!files || files.length === 0 || uploading) return;
    setUploading(true);
    setError(null);
    try {
      await api.uploadClips(Array.from(files));
      navigate("/");
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Upload failed.");
    } finally {
      setUploading(false);
    }
  };

  return (
    <form
      className="upload"
      onSubmit={(e) => {
        e.preventDefault();
        void upload(inputRef.current?.files ?? null);
      }}
    >
      <div
        className={dragging ? "dropzone drag" : "dropzone"}
        onDragEnter={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragOver={(e) => e.preventDefault()}
        onDragLeave={(e) => {
          e.preventDefault();
          setDragging(false);
        }}
        onDrop={(e) => {
          e.preventDefault();
          setDragging(false);
          void upload(e.dataTransfer.files);
        }}
      >
        <label>
          Audio files
          <input
            ref={inputRef}
            type="file"
            name="audio"
            accept="audio/*"
            multiple
            disabled={uploading}
          />
        </label>
        <button type="submit" disabled={uploading}>
          {uploading ? "Uploading…" : "Upload"}
        </button>
        <p className="hint">
          Drag &amp; drop files here, or choose several at once. Each clip is
          named from its filename and can be renamed afterwards.
        </p>
        {error !== null && <p className="error">{error}</p>}
      </div>
    </form>
  );
}

/** `rows` with `clipId` moved to just before or after `targetId`. */
function moved(
  rows: ClipSummary[],
  clipId: number,
  targetId: number,
  after: boolean,
): ClipSummary[] {
  const clip = rows.find((r) => r.id === clipId);
  if (!clip || clipId === targetId) return rows;
  const rest = rows.filter((r) => r.id !== clipId);
  const index = rest.findIndex((r) => r.id === targetId);
  if (index === -1) return rows;
  const at = after ? index + 1 : index;
  return [...rest.slice(0, at), clip, ...rest.slice(at)];
}

const sameOrder = (a: ClipSummary[], b: ClipSummary[]) =>
  a.length === b.length && a.every((row, i) => row.id === b[i]?.id);

/**
 * A label's clips as its playlist: in the order the band dragged them
 * into, reorderable by anyone, and played as a queue.
 *
 * A row moves while it is dragged, so the list is always showing the
 * order a drop would produce. Only the drop is sent, as "after this
 * clip" rather than an index, and the server's answer is what the list
 * settles on. A failed move springs back.
 */
function Playlist({
  playlist,
  clips,
}: {
  playlist: PlaylistInfo;
  clips: ClipSummary[];
}) {
  const { playQueue, setQueueTracks } = usePlayer();
  const { navigate } = useRouter();
  const [rows, setRows] = useState(clips);
  const [shown, setShown] = useState(clips);
  // The clip being moved, and the order from before it started moving.
  const [moving, setMoving] = useState<{
    clipId: number;
    from: ClipSummary[];
  } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const label = playlist.labelName;

  // Fresh props — a navigation back here, or a refetch — replace the
  // list outright.
  if (shown !== clips) {
    setShown(clips);
    setRows(clips);
    setMoving(null);
  }

  // A queue playing from this label is this list: coming back to it
  // after labels were added elsewhere is how the queue learns of them.
  useEffect(() => {
    setQueueTracks(label, clips.map(trackFor));
  }, [label, clips, setQueueTracks]);

  const show = (next: ClipSummary[]) => {
    setRows(next);
    setQueueTracks(label, next.map(trackFor));
  };

  const commit = async (clipId: number, from: ClipSummary[], to: ClipSummary[]) => {
    setMoving(null);
    if (sameOrder(from, to)) return;
    const index = to.findIndex((r) => r.id === clipId);
    const afterClipId = to[index - 1]?.id ?? null;
    setMessage(null);
    show(to);
    try {
      const { order } = await api.reorderPlaylist(
        playlist.labelId,
        clipId,
        afterClipId,
      );
      const byId = new Map(to.map((r) => [r.id, r]));
      show(order.flatMap((id) => byId.get(id) ?? []));
    } catch (e) {
      show(from);
      if (e instanceof ApiError) {
        // The list this move was made against no longer exists — a clip
        // lost the label, or the label is gone. Show the one that does.
        setMessage(`${e.message} The playlist has been reloaded.`);
        navigate(window.location.pathname + window.location.search, {
          replace: true,
        });
      } else {
        setMessage("Couldn't reach the server, so the order is as it was.");
      }
    }
  };

  const onDragOver = (e: ReactDragEvent<HTMLLIElement>, targetId: number) => {
    if (moving === null) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const rect = e.currentTarget.getBoundingClientRect();
    const after = e.clientY > rect.top + rect.height / 2;
    const next = moved(rows, moving.clipId, targetId, after);
    if (!sameOrder(next, rows)) setRows(next);
  };

  const onHandleKey = (e: ReactKeyboardEvent<HTMLButtonElement>, clipId: number) => {
    const held = moving?.clipId === clipId ? moving : null;
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      if (held) void commit(clipId, held.from, rows);
      else setMoving({ clipId, from: rows });
      return;
    }
    if (!held) return;
    if (e.key === "Escape") {
      e.preventDefault();
      setRows(held.from);
      setMoving(null);
      return;
    }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const index = rows.findIndex((r) => r.id === clipId);
    const neighbour = rows[e.key === "ArrowUp" ? index - 1 : index + 1];
    if (neighbour) {
      setRows(moved(rows, clipId, neighbour.id, e.key === "ArrowDown"));
    }
  };

  const known = rows.filter((r) => r.durationSeconds !== null).length;
  const unknown = rows.length - known;

  return (
    <>
      <div className="playlist-head">
        <strong>{label}</strong>
        {" · "}
        {rows.length === 1 ? "1 clip" : `${rows.length} clips`}
        {known > 0 && ` · ${formatTime(playlist.totalSeconds)}`}
        {unknown > 0 && known > 0 && ` (${unknown} unknown)`}
      </div>
      {message !== null && (
        <p className="error" role="alert">
          {message}
        </p>
      )}
      <ul className="clip-list playlist">
        {rows.map((clip) => (
          <ClipCard
            key={clip.id}
            clip={clip}
            onPlay={() => playQueue(rows.map(trackFor), clip.id, label)}
            dragging={moving?.clipId === clip.id}
            onDragOver={(e) => onDragOver(e, clip.id)}
            // The move is committed on dragend, which follows every drop.
            onDrop={(e) => e.preventDefault()}
            handle={
              <button
                type="button"
                className="drag-handle"
                draggable
                aria-label={`Move ${clip.name}`}
                aria-pressed={moving?.clipId === clip.id}
                title="Drag to reorder, or press Space and use the arrow keys"
                onDragStart={(e) => {
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", String(clip.id));
                  const row = e.currentTarget.closest("li");
                  if (row) e.dataTransfer.setDragImage(row, 16, 16);
                  setMoving({ clipId: clip.id, from: rows });
                }}
                onDragEnd={() => {
                  // Dropped outside the list: the row stays where it was
                  // last dragged to, and that is the move.
                  if (moving?.clipId === clip.id) {
                    void commit(clip.id, moving.from, rows);
                  }
                }}
                onKeyDown={(e) => onHandleKey(e, clip.id)}
                onBlur={() => {
                  if (moving?.clipId === clip.id) {
                    void commit(clip.id, moving.from, rows);
                  }
                }}
              >
                ⠿
              </button>
            }
          />
        ))}
      </ul>
    </>
  );
}

function ClipCard({
  clip,
  onPlay,
  handle,
  dragging = false,
  onDragOver,
  onDrop,
}: {
  clip: ClipSummary;
  /** Instead of playing the clip on its own. */
  onPlay?: () => void;
  /** The drag handle, in a playlist view. */
  handle?: ReactNode;
  dragging?: boolean;
  onDragOver?: (e: ReactDragEvent<HTMLLIElement>) => void;
  onDrop?: (e: ReactDragEvent<HTMLLIElement>) => void;
}) {
  const player = usePlayer();
  const isCurrent = player.track?.clipId === clip.id;
  const isPlaying = isCurrent && player.isPlaying;

  return (
    <li
      className={dragging ? "clip-card dragging" : "clip-card"}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      {handle}
      <div className="clip-body">
        <div className="clip-head">
          <a className="clip-name" href={`/clips/${clip.id}`}>
            {clip.name}
          </a>
          <span className="meta">
            {clip.recordingDate !== null && `recorded ${clip.recordingDate} · `}
            uploaded by {clip.uploader} on {clip.uploadedAt}
          </span>
        </div>

        <div className="mini-player">
          <button
            type="button"
            className="play"
            aria-label={isPlaying ? "Pause" : "Play"}
            onClick={onPlay ?? (() => player.play(trackFor(clip)))}
          >
            {isPlaying ? "⏸" : "▶"}
          </button>
          <Waveform
            peaks={clip.peaks}
            progress={isCurrent ? player.progress : 0}
            height={48}
            onSeek={isCurrent ? player.seek : undefined}
          />
          <span className="time">
            {clip.durationSeconds !== null ? formatTime(clip.durationSeconds) : ""}
          </span>
          <a
            className="download"
            href={`/clips/${clip.id}/audio?download=1`}
            download={clip.originalFilename}
            title="Download"
          >
            ⤓
          </a>
        </div>

        {clip.labels.length > 0 && (
          <div className="labels">
            {clip.labels.map((label) => (
              <a className="label" href={label.href} key={label.name}>
                {label.name}
              </a>
            ))}
          </div>
        )}
      </div>
    </li>
  );
}
