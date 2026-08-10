import { useEffect, useRef, useState } from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";
import { Waveform } from "../components/Waveform";
import { WikiPanel } from "../components/WikiPanel";
import { formatTime } from "../format";
import { trackFor, usePlayer } from "../player";
import { useRouter } from "../router";
import type { ClipSummary, IndexProps } from "../types";

export default function IndexPage({
  username,
  clips,
  activeFilters,
  activeWikis,
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

function ClipCard({ clip }: { clip: ClipSummary }) {
  const player = usePlayer();
  const isCurrent = player.track?.clipId === clip.id;
  const isPlaying = isCurrent && player.isPlaying;

  return (
    <li className="clip-card">
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
          onClick={() => player.play(trackFor(clip))}
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
    </li>
  );
}
