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
  /** Progress through the current track, 0–1, for waveform previews. */
  progress: number;
  /** Load a clip into the bar and start it. Re-playing toggles instead. */
  play: (track: Track) => void;
  /** Play/pause whatever is loaded. */
  toggle: () => void;
  /** Seek the current track, as a 0–1 fraction. Ignored if not loaded. */
  seek: (fraction: number) => void;
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

export function PlayerProvider({ children }: { children: ReactNode }) {
  const [track, setTrack] = useState<Track | null>(null);
  const [isPlaying, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const waveSurferRef = useRef<WaveSurfer | null>(null);
  // Mirrors `track` so `play` can compare against it without taking a
  // dependency on it — a state updater is the wrong place to do the
  // comparison, since React is free to run updaters more than once.
  const trackRef = useRef<Track | null>(null);
  trackRef.current = track;

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
    if (track.peaks && track.durationSeconds) {
      // Draw from the stored peaks and let an <audio preload="none">
      // fetch the file only once playback actually starts.
      const media = new Audio();
      media.preload = "none";
      media.src = track.audioUrl;
      ws = WaveSurfer.create({
        ...base,
        media,
        peaks: [track.peaks],
        duration: track.durationSeconds,
      });
      setDuration(track.durationSeconds);
      void ws.play();
    } else {
      // No peaks (an older upload, or a file symphonia couldn't read):
      // wavesurfer fetches and decodes, then we start.
      ws = WaveSurfer.create({ ...base, url: track.audioUrl });
      ws.on("ready", (d) => {
        setDuration(d);
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
    };
  }, [track]);

  const play = useCallback((next: Track) => {
    // Re-pressing play on the loaded clip toggles it rather than
    // rebuilding the instance and losing the position.
    if (trackRef.current?.clipId === next.clipId) {
      void waveSurferRef.current?.playPause();
      return;
    }
    setTrack(next);
  }, []);

  const toggle = useCallback(() => {
    void waveSurferRef.current?.playPause();
  }, []);

  const seek = useCallback((fraction: number) => {
    waveSurferRef.current?.seekTo(Math.min(1, Math.max(0, fraction)));
  }, []);

  const stop = useCallback(() => setTrack(null), []);

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
      progress: duration > 0 ? currentTime / duration : 0,
      play,
      toggle,
      seek,
      stop,
      rename,
    }),
    [
      track,
      isPlaying,
      currentTime,
      duration,
      play,
      toggle,
      seek,
      stop,
      rename,
    ],
  );

  return (
    <PlayerContext.Provider value={value}>
      {children}
      <PlayerBar containerRef={containerRef} />
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
}: {
  containerRef: RefObject<HTMLDivElement | null>;
}) {
  const { track, isPlaying, currentTime, toggle } = usePlayer();

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
        <a className="gp-name" href={track.clipHref}>
          {track.name}
        </a>
      )}
      {track && <span className="gp-time">{formatTime(currentTime)}</span>}
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
