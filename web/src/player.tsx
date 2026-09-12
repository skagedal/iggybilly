import {
  createContext,
  useCallback,
  useContext,
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
 */

export interface Track {
  clipId: number;
  name: string;
  audioUrl: string;
  clipHref: string;
  downloadUrl: string;
  downloadName: string;
  /** Precomputed peaks, or null when the server couldn't decode. */
  peaks: number[] | null;
  durationSeconds: number | null;
}

/** How far the pane's skip buttons jump. */
const SKIP_SECONDS = 10;

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
  originalFilename: string;
  peaks: number[] | null;
  durationSeconds: number | null;
}): Track {
  return {
    clipId: clip.id,
    name: clip.name,
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
  isPlaying: boolean;
  currentTime: number;
  /** The clip's length in seconds, or 0 while it isn't known. */
  duration: number;
  /** Progress through the current track, 0–1, for waveform previews. */
  progress: number;
  /** Whether the clip starts again instead of ending. */
  repeat: boolean;
  /** Load a clip into the bar and start it. Re-playing toggles instead. */
  play: (track: Track) => void;
  /** Play/pause whatever is loaded. */
  toggle: () => void;
  /** Seek the current track, as a 0–1 fraction. Ignored if not loaded. */
  seek: (fraction: number) => void;
  /** Jump by a number of seconds, clamped to the clip. Negative goes back. */
  skip: (seconds: number) => void;
  /** Turn looping on or off. Remembered for next time. */
  setRepeat: (repeat: boolean) => void;
  /** Drop the current track — used when its clip is deleted. */
  stop: () => void;
  /** Keep the bar's caption honest when a playing clip is renamed. */
  rename: (clipId: number, name: string) => void;
}

const PlayerContext = createContext<PlayerContextValue | null>(null);

export function usePlayer(): PlayerContextValue {
  const ctx = useContext(PlayerContext);
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

export function PlayerProvider({ children }: { children: ReactNode }) {
  const [track, setTrack] = useState<Track | null>(null);
  const [isPlaying, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [repeat, setRepeatState] = useState(storedRepeat);
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

  // Build (and rebuild) the instance whenever the loaded track changes.
  // The container belongs to the bar, which never unmounts, so this is
  // the only thing that ever tears the player down.
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

    ws.on("timeupdate", setCurrentTime);
    ws.on("play", () => setPlaying(true));
    ws.on("pause", () => setPlaying(false));
    ws.on("finish", () => setPlaying(false));

    waveSurferRef.current = ws;
    setCurrentTime(0);

    return () => {
      waveSurferRef.current = null;
      setPlaying(false);
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
  // same thing as a bar-length riff played twice. It also means `finish`
  // never fires while repeat is on, so nothing has to undo the pause.
  //
  // Separate from the effect above so that turning repeat on does not
  // rebuild the player and lose your place. `track` is a dependency
  // because a rebuilt player has its own element, which starts unlooped.
  useEffect(() => {
    const media = waveSurferRef.current?.getMediaElement();
    if (media) media.loop = repeat;
  }, [repeat, track]);

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
      setTrack(next);
    },
    [track],
  );

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

  const setRepeat = useCallback((next: boolean) => {
    setRepeatState(next);
    try {
      window.localStorage.setItem(REPEAT_KEY, next ? "1" : "0");
    } catch {
      // See storedRepeat: not being able to remember it is survivable.
    }
  }, []);

  // Closing the pane belongs here rather than in an effect watching for
  // a null track: this is the only way a track becomes null, and a pane
  // for a clip that has just been deleted has nothing to show.
  const stop = useCallback(() => {
    setTrack(null);
    setExpanded(false);
  }, []);

  const rename = useCallback((clipId: number, name: string) => {
    setTrack((current) =>
      current && current.clipId === clipId ? { ...current, name } : current,
    );
  }, []);

  const value = useMemo<PlayerContextValue>(
    () => ({
      track,
      isPlaying,
      currentTime,
      duration,
      progress: duration > 0 ? currentTime / duration : 0,
      repeat,
      play,
      toggle,
      seek,
      skip,
      setRepeat,
      stop,
      rename,
    }),
    [
      track,
      isPlaying,
      currentTime,
      duration,
      repeat,
      play,
      toggle,
      seek,
      skip,
      setRepeat,
      stop,
      rename,
    ],
  );

  return (
    <PlayerContext.Provider value={value}>
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
    </PlayerContext.Provider>
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
 * decisions that do not fit on one line of the bar.
 *
 * Its waveform is a canvas drawn from the stored peaks, not the bar's
 * WaveSurfer instance — that one is bound to a node which must not move.
 * Drawing a second picture of the same peaks costs one canvas and keeps
 * the player playing.
 */
function PlayerPane({ onClose }: { onClose: () => void }) {
  const {
    track,
    isPlaying,
    currentTime,
    duration,
    progress,
    repeat,
    toggle,
    seek,
    skip,
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

  return (
    <div className="player-pane" role="dialog" aria-label="Player">
      <div className="pp-head">
        <span className="pp-name">{track.name}</span>
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
        <button
          type="button"
          className="pp-play"
          aria-label={isPlaying ? "Pause" : "Play"}
          onClick={toggle}
        >
          {isPlaying ? "⏸" : "▶"}
        </button>
        <button
          type="button"
          className="pp-skip"
          title={`Forward ${SKIP_SECONDS} seconds`}
          onClick={() => skip(SKIP_SECONDS)}
        >
          +{SKIP_SECONDS}s
        </button>
        <a className="pp-open" href={track.clipHref}>
          Open clip
        </a>
      </div>
    </div>
  );
}
