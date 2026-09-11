import 'package:flutter/material.dart';

/// A clip's waveform, drawn from the peaks the server computed at
/// upload.
///
/// Drawn rather than played: a list of thirty clips would otherwise be
/// thirty media players, which is the mistake the web version made
/// before it drew its rows on a canvas. The peaks are already in the
/// clip payload, so a row costs one paint and no network.
class Waveform extends StatelessWidget {
  const Waveform({
    super.key,
    required this.peaks,
    this.progress = 0,
    this.height = 36,
    this.onSeek,
  });

  /// Normalised, 0 to 1. Null when the server could not decode the file,
  /// which draws a flat line rather than an empty space — the clip still
  /// plays, and the row should still look like a clip.
  final List<double>? peaks;

  /// How far through, 0 to 1. Bars before it are drawn in the played
  /// colour.
  final double progress;

  final double height;

  /// Called with a 0..1 position when the user taps or drags. Null makes
  /// the waveform a picture rather than a control.
  final ValueChanged<double>? onSeek;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final painter = _WaveformPainter(
      peaks: peaks,
      progress: progress,
      playedColor: scheme.primary,
      unplayedColor: scheme.outlineVariant,
    );

    final canvas = SizedBox(
      height: height,
      width: double.infinity,
      child: CustomPaint(painter: painter),
    );

    if (onSeek == null) return canvas;

    return LayoutBuilder(
      builder: (context, constraints) => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: (d) => _seek(d.localPosition.dx, constraints.maxWidth),
        onHorizontalDragUpdate: (d) =>
            _seek(d.localPosition.dx, constraints.maxWidth),
        child: canvas,
      ),
    );
  }

  void _seek(double dx, double width) {
    if (width <= 0) return;
    onSeek!((dx / width).clamp(0.0, 1.0));
  }
}

class _WaveformPainter extends CustomPainter {
  _WaveformPainter({
    required this.peaks,
    required this.progress,
    required this.playedColor,
    required this.unplayedColor,
  });

  final List<double>? peaks;
  final double progress;
  final Color playedColor;
  final Color unplayedColor;

  /// Bar plus gap, in logical pixels. Wide enough that a bar is visible
  /// on a phone, narrow enough that a few seconds of audio still reads
  /// as a shape.
  static const _barPitch = 3.0;
  static const _barWidth = 2.0;

  @override
  void paint(Canvas canvas, Size size) {
    final source = peaks;
    final middle = size.height / 2;
    final playedUpTo = size.width * progress;

    if (source == null || source.isEmpty) {
      // No peaks: a centre line, so the row keeps its shape and the
      // progress is still legible.
      final paint = Paint()..strokeWidth = 1;
      canvas.drawLine(
        Offset(0, middle),
        Offset(playedUpTo, middle),
        paint..color = playedColor,
      );
      canvas.drawLine(
        Offset(playedUpTo, middle),
        Offset(size.width, middle),
        paint..color = unplayedColor,
      );
      return;
    }

    final bars = (size.width / _barPitch).floor();
    if (bars <= 0) return;

    final paint = Paint()
      ..strokeWidth = _barWidth
      ..strokeCap = StrokeCap.round;

    for (var i = 0; i < bars; i++) {
      // Take the loudest peak in the slice this bar covers, rather than
      // sampling one: a transient that lands between samples would
      // otherwise disappear, and a drum hit is the thing you look for.
      final from = (i * source.length / bars).floor();
      final to = (((i + 1) * source.length) / bars).ceil().clamp(from + 1, source.length);
      var peak = 0.0;
      for (var j = from; j < to; j++) {
        final v = source[j].abs();
        if (v > peak) peak = v;
      }

      final x = i * _barPitch + _barWidth / 2;
      // A floor of one pixel, so silence is a line rather than a gap.
      final half = (peak.clamp(0.0, 1.0) * middle).clamp(0.5, middle);
      paint.color = x <= playedUpTo ? playedColor : unplayedColor;
      canvas.drawLine(Offset(x, middle - half), Offset(x, middle + half), paint);
    }
  }

  @override
  bool shouldRepaint(_WaveformPainter old) =>
      old.progress != progress ||
      old.peaks != peaks ||
      old.playedColor != playedColor ||
      old.unplayedColor != unplayedColor;
}
