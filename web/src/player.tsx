import {
  createContext,
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ReactNode, RefObject } from "react";
import WaveSurfer from "wavesurfer.js";

import { Waveform } from "./components/Waveform";
import { formatTime } from "./format";

/**
 * The one audio player in the app.
 *
 * There is exactly one WaveSurfer instance, living in a bar that is
 * mounted above the router's outlet and never unmounted. Playing a clip
 * anywhere hands it to this bar; playing another takes the bar over.
 * Because client-side navigation keeps the document alive, the audio
 * keeps going as you move between pages.
 *
 * Every playback is a queue. Most queues hold one track; pressing play
 * in a playlist view queues the whole label.
 */

export interface Track {
  clipId: number;
  name: string;
  uploader: string;
  recordingDate: string | null;
  audioUrl: string;
  clipHref: string;
  downloadUrl: string;
  downloadName: string;
  /** Precomputed peaks, or null when the server couldn't decode. */
  peaks: number[] | null;
  durationSeconds: number | null;
}

/**
 * What plays next. The current track is the player's `track`, found in
 * `tracks` by id rather than held as an index, so a reorder arriving
 * under a playing queue needs no repair.
 */
export interface Queue {
  /** The label this queue is the playlist of, or null for a lone clip. */
  source: string | null;
  tracks: Track[];
  /**
   * Where to carry on once the playing clip has left `tracks` — its
   * label removed while it played: the first of its old successors
   * still in the list.
   */
  resumeAt: number | null;
}

/** How far the pane's skip buttons jump. */
const SKIP_SECONDS = 10;

/** Past this far into a track, previous restarts it instead of going back. */
const RESTART_SECONDS = 3;

/** Where the repeat setting lives between visits. */
const REPEAT_KEY = "iggybilly.repeat";

/**
 * Build a Track from a clip. Both the list and the detail page carry the
 * fields the player needs, so this takes the overlap rather than either
 * concrete shape.
 */
export function trackFor(clip: {
  id: number;
  name: string;
  uploader: string;
  recordingDate: string | null;
  originalFilename: string;
  peaks: number[] | null;
  durationSeconds: number | null;
}): Track {
  return {
    clipId: clip.id,
    name: clip.name,
    uploader: clip.uploader,
    recordingDate: clip.recordingDate,
    audioUrl: `/clips/${clip.id}/audio`,
    clipHref: `/clips/${clip.id}`,
    downloadUrl: `/clips/${clip.id}/audio?download=1`,
    downloadName: clip.originalFilename,
    peaks: clip.peaks,
    durationSeconds: clip.durationSeconds,
  };
}

interface PlayerContextValue {
  track: Track | null;
  queue: Queue;
  isPlaying: boolean;
  currentTime: number;
  /** The clip's length in seconds, or 0 while it isn't known. */
  duration: number;
  /** Progress through the current track, 0–1, for waveform previews. */
  progress: number;
  /** Whether the queue starts again instead of ending. */
  repeat: boolean;
  /** Why playback stopped, when it stopped on its own. */
  error: string | null;
  /** Load a clip as a queue of one and start it. Re-playing toggles instead. */
  play: (track: Track) => void;
  /** Queue a label's tracks and start at `clipId`. Re-playing toggles. */
  playQueue: (tracks: Track[], clipId: number, source: string) => void;
  /** Play the track after this one; wraps when repeat is on. */
  next: () => void;
  /** Go back a track, or restart this one if it is past its opening. */
  previous: () => void;
  /** Play a track already in the queue. */
  jumpTo: (clipId: number) => void;
  /**
   * Replace what is queued without touching what is playing — if the
   * queue is the playlist of `source`, and otherwise nothing.
   */
  setQueueTracks: (source: string, tracks: Track[]) => void;
  /**
   * Keep a playing label queue in step with a clip's labels: the clip
   * joins the end when it gains the label, and leaves when it loses it.
   */
  syncLabels: (track: Track, labelNames: string[]) => void;
  /** Play/pause whatever is loaded. */
  toggle: () => void;
  /** Seek the current track, as a 0–1 fraction. Ignored if not loaded. */
  seek: (fraction: number) => void;
  /** Jump by a number of seconds, clamped to the clip. Negative goes back. */
  skip: (seconds: number) => void;
  /** Turn repeat on or off. Remembered for next time. */
  setRepeat: (repeat: boolean) => void;
  /** A clip was deleted: stop if it is playing, else drop it from the queue. */
  forget: (clipId: number) => void;
  /** Keep the bar's caption honest when a playing clip is renamed. */
  rename: (clipId: number, name: string) => void;
}

const PlayerContext = createContext<PlayerContextValue | null>(null);

export function usePlayer(): PlayerContextValue {
  const ctx = use(PlayerContext);
  if (!ctx) throw new Error("usePlayer must be used inside <PlayerProvider>");
  return ctx;
}

/** The stored repeat setting, or false if there isn't one or we can't read it. */
function storedRepeat(): boolean {
  try {
    return window.localStorage.getItem(REPEAT_KEY) === "1";
  } catch {
    // Private browsing with storage blocked. Repeat then lasts one visit,
    // which is better than the page failing to load.
    return false;
  }
}

const EMPTY_QUEUE: Queue = { source: null, tracks: [], resumeAt: null };

/** `queue` with `tracks` swapped in, keeping track of where `current` was. */
function withTracks(queue: Queue, tracks: Track[], current: number | null): Queue {
  const kept = new Set(tracks.map((t) => t.clipId));
  let { resumeAt } = queue;
  const index = queue.tracks.findIndex((t) => t.clipId === current);
  if (current !== null && index !== -1 && !kept.has(current)) {
    resumeAt =
      queue.tracks.slice(index + 1).find((t) => kept.has(t.clipId))?.clipId ??
      null;
  }
  return { ...queue, tracks, resumeAt };
}

/** The track to play after `current`, or null when the queue has run out. */
function following(
  queue: Queue,
  current: number | null,
  repeat: boolean,
): Track | null {
  const { tracks, resumeAt } = queue;
  if (tracks.length === 0) return null;
  const index = tracks.findIndex((t) => t.clipId === current);
  if (index === -1) {
    const resume = tracks.find((t) => t.clipId === resumeAt);
    if (resume) return resume;
    return repeat ? (tracks[0] ?? null) : null;
  }
  return tracks[index + 1] ?? (repeat ? (tracks[0] ?? null) : null);
}

export function PlayerProvider({ children }: { children: ReactNode }) {
  const [track, setTrack] = useState<Track | null>(null);
  const [queue, setQueue] = useState<Queue>(EMPTY_QUEUE);
  const [isPlaying, setIsPlaying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The position is stored together with the clip it was measured in, so
  // loading another clip reads as 0 without an effect having to reset it.
  const [played, setPlayed] = useState<{
    clipId: number | null;
    seconds: number;
  }>({ clipId: null, seconds: 0 });
  const currentTime = played.clipId === track?.clipId ? played.seconds : 0;
  const [repeat, setRepeat] = useState(storedRepeat);
  // Whether the detailed pane is up. Local to the provider rather than in
  // the context: only the bar opens it and only the pane closes it.
  const [expanded, setExpanded] = useState(false);
  // Only for clips the server couldn't decode: wavesurfer reports their
  // duration once it has fetched the file. Everything else carries its
  // duration in the track, so the length is derived, not stored twice.
  const [decodedDuration, setDecodedDuration] = useState(0);
  const duration = track?.durationSeconds ?? decodedDuration;

  const containerRef = useRef<HTMLDivElement | null>(null);
  const waveSurferRef = useRef<WaveSurfer | null>(null);
  // Tracks that failed to load in a row. A whole pass of them stops
  // playback rather than spinning through a dead list.
  const failuresRef = useRef(0);
  // For the queue edits, which arrive from pages and must not change
  // identity every time the track does.
  const trackRef = useRef<Track | null>(null);
  useEffect(() => {
    trackRef.current = track;
  }, [track]);

  // The instance's event handlers outlive renders, so they reach the
  // queue through these rather than through a stale closure.
  const onFinishRef = useRef<() => void>(() => {});
  const onErrorRef = useRef<() => void>(() => {});

  // Build (and rebuild) the instance whenever the loaded track changes.
  // The container belongs to the bar, which never unmounts, so this is
  // the only thing that ever tears the player down — and advancing the
  // queue is a change of track, so it goes through here too.
  useEffect(() => {
    const container = containerRef.current;
    if (!container || !track) return;

    const base = {
      container,
      waveColor: "#aaa",
      progressColor: "#2a8055",
      height: 40,
    };

    let ws: WaveSurfer;
    // Held in the effect's scope so the cleanup below can stop it.
    // wavesurfer's own destroy() bails out before pausing a media element
    // it did not create — `if (this.isExternalMedia) return`, in
    // Player.destroy — so one we supply plays on with nothing left
    // holding a reference to stop it. That is two clips at once the
    // moment you start a second.
    let media: HTMLAudioElement | null = null;
    if (track.peaks && track.durationSeconds) {
      // Draw from the stored peaks and let an <audio preload="none">
      // fetch the file only once playback actually starts.
      media = new Audio();
      media.preload = "none";
      media.src = track.audioUrl;
      ws = WaveSurfer.create({
        ...base,
        media,
        peaks: [track.peaks],
        duration: track.durationSeconds,
      });
      void ws.play();
    } else {
      // No peaks (an older upload, or a file symphonia couldn't read):
      // wavesurfer fetches and decodes, then we start.
      ws = WaveSurfer.create({ ...base, url: track.audioUrl });
      ws.on("ready", (d) => {
        setDecodedDuration(d);
        void ws.play();
      });
    }

    ws.on("timeupdate", (seconds) => {
      // Audio actually moving, not just `play` — which fires before a
      // missing file has had the chance to fail.
      if (seconds > 0) failuresRef.current = 0;
      setPlayed({ clipId: track.clipId, seconds });
    });
    ws.on("play", () => setIsPlaying(true));
    ws.on("pause", () => setIsPlaying(false));
    ws.on("finish", () => onFinishRef.current());
    // A failed fetch or decode, or the media element refusing the file.
    ws.on("error", () => onErrorRef.current());

    waveSurferRef.current = ws;

    return () => {
      waveSurferRef.current = null;
      setIsPlaying(false);
      ws.destroy();
      if (media) {
        media.pause();
        media.removeAttribute("src");
        // Resets the element and drops the fetch still in flight.
        media.load();
      }
    };
  }, [track]);

  // Looping is the media element's own, which makes it gapless — a
  // bar-length riff played twice with a hole in the middle is not the
  // same thing as a bar-length riff played twice. So the element loops
  // itself whenever repeat is on and the queue is this one track; only a
  // real playlist wraps by advancing, and there the seam is a track
  // change anyway. With the element looping, `finish` never fires.
  //
  // Separate from the effect above so that turning repeat on does not
  // rebuild the player and lose your place. `track` is a dependency
  // because a rebuilt player has its own element, which starts unlooped;
  // the queue is one because the rule depends on its length.
  const loopsItself =
    repeat &&
    queue.tracks.length === 1 &&
    queue.tracks[0]?.clipId === track?.clipId;
  useEffect(() => {
    const media = waveSurferRef.current?.getMediaElement();
    if (media) media.loop = loopsItself;
  }, [loopsItself, track]);

  /** Start `next`, or wind the current one back if it is the same clip. */
  const start = useCallback(
    (next: Track) => {
      setError(null);
      setQueue((q) => (q.resumeAt === null ? q : { ...q, resumeAt: null }));
      if (next.clipId === track?.clipId) {
        const ws = waveSurferRef.current;
        ws?.setTime(0);
        void ws?.play();
        return;
      }
      setTrack(next);
    },
    [track],
  );

  const advance = useCallback(() => {
    const next = following(queue, track?.clipId ?? null, repeat);
    if (next) {
      start(next);
      return;
    }
    // Leave the last track loaded and wound back, so the obvious next
    // gesture — press play again — works.
    const ws = waveSurferRef.current;
    ws?.pause();
    ws?.setTime(0);
  }, [queue, track, repeat, start]);

  const stop = useCallback(() => {
    setTrack(null);
    setQueue(EMPTY_QUEUE);
    // Dropping the position too: stopping and then playing the same clip
    // rebuilds the instance at zero, and a kept position would be read as
    // still belonging to it until the first timeupdate.
    setPlayed({ clipId: null, seconds: 0 });
    // A pane for a clip that is gone has nothing to show.
    setExpanded(false);
  }, []);

  useEffect(() => {
    onFinishRef.current = advance;
    onErrorRef.current = () => {
      // A clip deleted after it was queued, or a file the browser will
      // not decode. Skip it, unless every track has now failed in turn.
      failuresRef.current += 1;
      const next = following(queue, track?.clipId ?? null, true);
      if (
        !next ||
        next.clipId === track?.clipId ||
        failuresRef.current >= queue.tracks.length
      ) {
        failuresRef.current = 0;
        stop();
        setError("That clip could not be played.");
        return;
      }
      start(next);
    };
  }, [advance, queue, track, start, stop]);

  const play = useCallback(
    (next: Track) => {
      // Re-pressing play on the loaded clip toggles it rather than
      // rebuilding the instance and losing the position. The comparison
      // belongs here and not in a state updater, which React is free to
      // run more than once.
      if (track?.clipId === next.clipId) {
        void waveSurferRef.current?.playPause();
        return;
      }
      setQueue({ source: null, tracks: [next], resumeAt: null });
      start(next);
    },
    [track, start],
  );

  const playQueue = useCallback(
    (tracks: Track[], clipId: number, source: string) => {
      const next = tracks.find((t) => t.clipId === clipId);
      if (!next) return;
      setQueue({ source, tracks, resumeAt: null });
      if (track?.clipId === clipId) {
        // The same clip, perhaps now from its playlist: keep the place.
        void waveSurferRef.current?.playPause();
        return;
      }
      start(next);
    },
    [track, start],
  );

  const previous = useCallback(() => {
    const ws = waveSurferRef.current;
    const index = queue.tracks.findIndex((t) => t.clipId === track?.clipId);
    const before = queue.tracks[index - 1];
    if ((ws && ws.getCurrentTime() > RESTART_SECONDS) || !before) {
      ws?.setTime(0);
      return;
    }
    start(before);
  }, [queue, track, start]);

  const jumpTo = useCallback(
    (clipId: number) => {
      const next = queue.tracks.find((t) => t.clipId === clipId);
      if (next) start(next);
    },
    [queue, start],
  );

  const setQueueTracks = useCallback((source: string, tracks: Track[]) => {
    setQueue((q) =>
      q.source === source
        ? withTracks(q, tracks, trackRef.current?.clipId ?? null)
        : q,
    );
  }, []);

  const syncLabels = useCallback((clip: Track, labelNames: string[]) => {
    setQueue((q) => {
      if (q.source === null) return q;
      const queued = q.tracks.some((t) => t.clipId === clip.clipId);
      const labelled = labelNames.includes(q.source);
      const current = trackRef.current?.clipId ?? null;
      if (labelled && !queued) return withTracks(q, [...q.tracks, clip], current);
      if (!labelled && queued) {
        return withTracks(
          q,
          q.tracks.filter((t) => t.clipId !== clip.clipId),
          current,
        );
      }
      return q;
    });
  }, []);

  const toggle = useCallback(() => {
    void waveSurferRef.current?.playPause();
  }, []);

  const seek = useCallback((fraction: number) => {
    waveSurferRef.current?.seekTo(Math.min(1, Math.max(0, fraction)));
  }, []);

  const skip = useCallback((seconds: number) => {
    const ws = waveSurferRef.current;
    if (!ws) return;
    const total = ws.getDuration();
    const target = ws.getCurrentTime() + seconds;
    ws.setTime(Math.min(total || target, Math.max(0, target)));
  }, []);

  const changeRepeat = useCallback((next: boolean) => {
    setRepeat(next);
    try {
      window.localStorage.setItem(REPEAT_KEY, next ? "1" : "0");
    } catch {
      // See storedRepeat: not being able to remember it is survivable.
    }
  }, []);

  const forget = useCallback(
    (clipId: number) => {
      if (trackRef.current?.clipId === clipId) {
        stop();
        return;
      }
      setQueue((q) =>
        q.tracks.some((t) => t.clipId === clipId)
          ? withTracks(q, q.tracks.filter((t) => t.clipId !== clipId), null)
          : q,
      );
    },
    [stop],
  );

  const rename = useCallback((clipId: number, name: string) => {
    setTrack((current) =>
      current && current.clipId === clipId ? { ...current, name } : current,
    );
    setQueue((q) => ({
      ...q,
      tracks: q.tracks.map((t) => (t.clipId === clipId ? { ...t, name } : t)),
    }));
  }, []);

  const value = useMemo<PlayerContextValue>(
    () => ({
      track,
      queue,
      isPlaying,
      currentTime,
      duration,
      progress: duration > 0 ? currentTime / duration : 0,
      repeat,
      error,
      play,
      playQueue,
      next: advance,
      previous,
      jumpTo,
      setQueueTracks,
      syncLabels,
      toggle,
      seek,
      skip,
      setRepeat: changeRepeat,
      forget,
      rename,
    }),
    [
      track,
      queue,
      isPlaying,
      currentTime,
      duration,
      repeat,
      error,
      play,
      playQueue,
      advance,
      previous,
      jumpTo,
      setQueueTracks,
      syncLabels,
      toggle,
      seek,
      skip,
      changeRepeat,
      forget,
      rename,
    ],
  );

  return (
    <PlayerContext value={value}>
      {children}
      <PlayerBar
        containerRef={containerRef}
        expanded={expanded}
        onToggleExpanded={() => setExpanded((open) => !open)}
      />
      {/* After the bar, never before it: React matches children by
          position, so a slot that appears and disappears ahead of the bar
          would take the waveform host — the one node that has to outlive
          every change — down with it. */}
      {expanded && <PlayerPane onClose={() => setExpanded(false)} />}
      {error !== null && (
        <button
          type="button"
          className="player-error"
          role="alert"
          title="Dismiss"
          onClick={() => setError(null)}
        >
          {error}
        </button>
      )}
    </PlayerContext>
  );
}

/**
 * The bar itself, hidden until something is loaded.
 *
 * One structure in both states, deliberately. React matches children by
 * position, so returning a different tree when there's no track would
 * let it destroy and recreate the waveform host — the one node the whole
 * design needs to stay put, since the WaveSurfer instance is attached to
 * it and has to outlive every page swap. The `{track && …}` slots render
 * as nothing but still hold their positions, so the host is always the
 * second child and is never touched.
 */
function PlayerBar({
  containerRef,
  expanded,
  onToggleExpanded,
}: {
  containerRef: RefObject<HTMLDivElement | null>;
  expanded: boolean;
  onToggleExpanded: () => void;
}) {
  const { track, isPlaying, currentTime, repeat, toggle } = usePlayer();

  // Keep the page's last row clear of the bar.
  useEffect(() => {
    document.body.classList.toggle("has-player", track !== null);
    return () => document.body.classList.remove("has-player");
  }, [track]);

  return (
    <div className="global-player" hidden={track === null}>
      {track && (
        <button
          type="button"
          className="gp-play"
          aria-label={isPlaying ? "Pause" : "Play"}
          onClick={toggle}
        >
          {isPlaying ? "⏸" : "▶"}
        </button>
      )}
      <div className="gp-waveform" ref={containerRef} />
      {track && (
        // The name opens the whole player rather than the clip's page.
        // The page is a click away inside it, and the waveform beside
        // this still seeks — a bar you cannot scrub would be a worse bar.
        <button
          type="button"
          className="gp-name"
          aria-expanded={expanded}
          title="Show the player"
          onClick={onToggleExpanded}
        >
          {track.name}
          {repeat && (
            <span className="gp-repeat" aria-label="Repeating">
              ⟳
            </span>
          )}
        </button>
      )}
      {track && (
        <button
          type="button"
          className="gp-time"
          aria-expanded={expanded}
          title="Show the player"
          onClick={onToggleExpanded}
        >
          {formatTime(currentTime)}
        </button>
      )}
      {track && (
        <a
          className="gp-download"
          href={track.downloadUrl}
          download={track.downloadName}
          title="Download"
        >
          ⤓
        </a>
      )}
    </div>
  );
}

/**
 * The player, opened up: the whole waveform, both times, and the
 * decisions that do not fit on one line of the bar. Laid out as the
 * phone's sheet is.
 *
 * Its waveform is a canvas drawn from the stored peaks, not the bar's
 * WaveSurfer instance — that one is bound to a node which must not move.
 * Drawing a second picture of the same peaks costs one canvas and keeps
 * the player playing.
 */
function PlayerPane({ onClose }: { onClose: () => void }) {
  const {
    track,
    queue,
    isPlaying,
    currentTime,
    duration,
    progress,
    repeat,
    toggle,
    seek,
    skip,
    next,
    previous,
    setRepeat,
  } = usePlayer();

  // Escape closes it, as it would any other panel over the page.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  if (!track) return null;

  const subtitle = [
    track.uploader,
    track.recordingDate !== null ? `rec. ${track.recordingDate}` : null,
  ]
    .filter((part) => part)
    .join(" · ");
  const hasQueue = queue.tracks.length > 1;

  return (
    <div className="player-pane" role="dialog" aria-label="Player">
      <div className="pp-head">
        <div className="pp-title">
          <span className="pp-name">{track.name}</span>
          {subtitle && <span className="pp-subtitle">{subtitle}</span>}
        </div>
        <button
          type="button"
          className="pp-close"
          aria-label="Close the player"
          onClick={onClose}
        >
          ×
        </button>
      </div>

      <Waveform peaks={track.peaks} progress={progress} height={88} onSeek={seek} />

      <div className="pp-times">
        <span>{formatTime(currentTime)}</span>
        <span>{duration > 0 ? formatTime(duration) : "—"}</span>
      </div>

      <div className="pp-controls">
        <button
          type="button"
          className={repeat ? "pp-repeat on" : "pp-repeat"}
          aria-pressed={repeat}
          title={repeat ? "Stop repeating" : "Repeat"}
          onClick={() => setRepeat(!repeat)}
        >
          ⟳
        </button>
        <button
          type="button"
          className="pp-skip"
          title={`Back ${SKIP_SECONDS} seconds`}
          onClick={() => skip(-SKIP_SECONDS)}
        >
          −{SKIP_SECONDS}s
        </button>
        {hasQueue && (
          <button
            type="button"
            className="pp-step"
            aria-label="Previous"
            title="Previous"
            onClick={previous}
          >
            ⏮
          </button>
        )}
        <button
          type="button"
          className="pp-play"
          aria-label={isPlaying ? "Pause" : "Play"}
          onClick={toggle}
        >
          {isPlaying ? "⏸" : "▶"}
        </button>
        {hasQueue && (
          <button
            type="button"
            className="pp-step"
            aria-label="Next"
            title="Next"
            onClick={next}
          >
            ⏭
          </button>
        )}
        <button
          type="button"
          className="pp-skip"
          title={`Forward ${SKIP_SECONDS} seconds`}
          onClick={() => skip(SKIP_SECONDS)}
        >
          +{SKIP_SECONDS}s
        </button>
      </div>

      {queue.source !== null && <QueueList />}

      <a className="pp-open" href={track.clipHref}>
        Open clip
      </a>
    </div>
  );
}

/** "Playing from verse-1 — 3 of 8", opening into the tracks themselves. */
function QueueList() {
  const { track, queue, jumpTo } = usePlayer();
  const [open, setOpen] = useState(false);

  const index = queue.tracks.findIndex((t) => t.clipId === track?.clipId);

  return (
    <div className="pp-queue">
      <button
        type="button"
        className="pp-queue-toggle"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        Playing from <strong>{queue.source}</strong>
        {index !== -1 && ` — ${index + 1} of ${queue.tracks.length}`}
        <span className="pp-queue-caret">{open ? "▴" : "▾"}</span>
      </button>
      {open && (
        <ol className="pp-queue-list">
          {queue.tracks.map((t) => (
            <li key={t.clipId}>
              <button
                type="button"
                className={
                  t.clipId === track?.clipId ? "pp-queue-row current" : "pp-queue-row"
                }
                aria-current={t.clipId === track?.clipId}
                onClick={() => jumpTo(t.clipId)}
              >
                <span className="pp-queue-name">{t.name}</span>
                <span className="pp-queue-time">
                  {t.durationSeconds !== null ? formatTime(t.durationSeconds) : ""}
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
