/// Turning values into the strings the UI shows.
///
/// Plain Dart on purpose: this is the part most likely to be wrong at a
/// boundary — midnight, a clip under a second, yesterday — and it is
/// only cheap to test if nothing here touches a plugin or a widget.
library;

/// A playback position or clip length, as "m:ss", or "h:mm:ss" once it
/// runs past an hour. Never "0:60": the seconds are taken from the
/// remainder, not rounded independently.
String formatDuration(Duration d) {
  if (d.isNegative) return '0:00';
  final hours = d.inHours;
  final minutes = d.inMinutes.remainder(60);
  final seconds = d.inSeconds.remainder(60);
  final ss = seconds.toString().padLeft(2, '0');
  if (hours > 0) {
    return '$hours:${minutes.toString().padLeft(2, '0')}:$ss';
  }
  return '$minutes:$ss';
}

/// When something happened, relative to [now] where that reads better.
///
/// A band uploads a clip and looks at it minutes later, so the recent
/// past is worth spelling out. Past a week the actual date is more use
/// than a count of days.
String formatWhen(DateTime when, {DateTime? now}) {
  final reference = now ?? DateTime.now();
  final elapsed = reference.difference(when);

  if (elapsed.isNegative) return formatDate(when);
  if (elapsed.inMinutes < 1) return 'just now';
  if (elapsed.inMinutes < 60) {
    final m = elapsed.inMinutes;
    return '$m ${m == 1 ? 'minute' : 'minutes'} ago';
  }
  if (elapsed.inHours < 24) {
    final h = elapsed.inHours;
    return '$h ${h == 1 ? 'hour' : 'hours'} ago';
  }
  if (elapsed.inDays < 7) {
    final d = elapsed.inDays;
    return '$d ${d == 1 ? 'day' : 'days'} ago';
  }
  return formatDate(when);
}

/// ISO "YYYY-MM-DD". Unambiguous everywhere, which a band with members
/// in more than one country cares about more than it looking local.
String formatDate(DateTime when) {
  final m = when.month.toString().padLeft(2, '0');
  final d = when.day.toString().padLeft(2, '0');
  return '${when.year}-$m-$d';
}

/// "YYYY-MM-DD HH:MM" — used where the time of day matters, such as
/// telling two wiki revisions from the same day apart.
String formatDateTime(DateTime when) {
  final h = when.hour.toString().padLeft(2, '0');
  final min = when.minute.toString().padLeft(2, '0');
  return '${formatDate(when)} $h:$min';
}

/// A byte count for a human. Binary units, because that is what a file
/// manager on either platform will also say.
String formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  const units = ['kB', 'MB', 'GB'];
  var value = bytes / 1024;
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  // One decimal below ten, none above: "1.4 MB", but "312 kB".
  final text = value < 10 ? value.toStringAsFixed(1) : value.round().toString();
  return '$text ${units[unit]}';
}

/// A base URL typed by a person, as a URL.
///
/// People type "iggybilly.skagedal.tech", with a trailing slash, or with
/// a path they copied from a link. Returns null when there is nothing
/// usable, so the sign-in screen can say so before trying to connect.
Uri? parseServerUrl(String input) {
  var text = input.trim();
  if (text.isEmpty) return null;
  if (!text.contains('://')) text = 'https://$text';

  final parsed = Uri.tryParse(text);
  if (parsed == null) return null;
  if (parsed.host.isEmpty) return null;
  if (parsed.scheme != 'http' && parsed.scheme != 'https') return null;

  // Keep the port, drop everything after the origin: paths, queries and
  // fragments come from pasting a link to a clip, and every API path is
  // resolved from the origin.
  return Uri(scheme: parsed.scheme, host: parsed.host, port: parsed.hasPort ? parsed.port : null);
}
