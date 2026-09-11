import { useCallback, useEffect, useRef } from "react";

interface Props {
  /** Precomputed peaks, or null when the server couldn't decode. */
  peaks: number[] | null;
  /** How far through, 0–1. Only meaningful for the clip in the bar. */
  progress: number;
  height: number;
  /** Called with a 0–1 fraction when the user clicks to seek. */
  onSeek?: ((fraction: number) => void) | undefined;
}

const WAVE_COLOR = "#aaa";
const PROGRESS_COLOR = "#2a8055";

/**
 * A clip's waveform, drawn straight from the stored peaks.
 *
 * These used to be WaveSurfer instances — one per row, each holding its
 * own `<audio>`. Now that the global player owns playback, a row's
 * waveform only has to be a picture, so it's a canvas we draw ourselves:
 * no audio element, no decode, and a list of thirty clips costs thirty
 * bar charts instead of thirty media players.
 */
export function Waveform({ peaks, progress, height, onSeek }: Props) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Match the backing store to the CSS size and the display density,
    // or the bars come out blurry on a retina screen.
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    if (width === 0) return;
    if (canvas.width !== width * ratio || canvas.height !== height * ratio) {
      canvas.width = width * ratio;
      canvas.height = height * ratio;
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    if (!peaks || peaks.length === 0) {
      // Nothing decoded: a flat line reads better than empty space.
      ctx.fillStyle = WAVE_COLOR;
      ctx.fillRect(0, height / 2 - 0.5, width, 1);
      return;
    }

    const barWidth = 2;
    const gap = 1;
    const step = barWidth + gap;
    const bars = Math.max(1, Math.floor(width / step));
    const playedBars = Math.round(bars * progress);
    const mid = height / 2;

    for (let i = 0; i < bars; i++) {
      // Peaks rarely line up 1:1 with the bars we're drawing, so take
      // the loudest sample in each bar's slice — that keeps transients
      // visible instead of averaging them away.
      const from = Math.floor((i / bars) * peaks.length);
      const to = Math.max(from + 1, Math.floor(((i + 1) / bars) * peaks.length));
      let peak = 0;
      for (let j = from; j < to && j < peaks.length; j++) {
        const v = Math.abs(peaks[j] ?? 0);
        if (v > peak) peak = v;
      }

      const barHeight = Math.max(1, peak * (height - 2));
      ctx.fillStyle = i < playedBars ? PROGRESS_COLOR : WAVE_COLOR;
      ctx.fillRect(i * step, mid - barHeight / 2, barWidth, barHeight);
    }
  }, [peaks, progress, height]);

  useEffect(() => {
    draw();
  }, [draw]);

  // Redraw when the element changes width — the canvas backing store
  // doesn't scale with CSS, so a resize would otherwise stretch it.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [draw]);

  const seek = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!onSeek) return;
    const rect = e.currentTarget.getBoundingClientRect();
    onSeek((e.clientX - rect.left) / rect.width);
  };

  return (
    <canvas
      ref={canvasRef}
      className={onSeek ? "waveform seekable" : "waveform"}
      style={{ height }}
      onClick={onSeek ? seek : undefined}
    />
  );
}
