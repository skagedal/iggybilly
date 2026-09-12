import 'package:flutter_test/flutter_test.dart';
import 'package:iggybilly/src/format.dart';

void main() {
  group('formatDuration', () {
    test('counts minutes and seconds', () {
      expect(formatDuration(Duration.zero), '0:00');
      expect(formatDuration(const Duration(seconds: 9)), '0:09');
      expect(formatDuration(const Duration(seconds: 75)), '1:15');
    });

    test('never rolls over into a sixtieth second', () {
      expect(formatDuration(const Duration(seconds: 59)), '0:59');
      expect(formatDuration(const Duration(seconds: 60)), '1:00');
      expect(formatDuration(const Duration(seconds: 119)), '1:59');
    });

    test('grows an hours field only when there are hours', () {
      expect(formatDuration(const Duration(minutes: 59)), '59:00');
      expect(formatDuration(const Duration(hours: 1, seconds: 5)), '1:00:05');
      expect(
        formatDuration(const Duration(hours: 2, minutes: 3, seconds: 4)),
        '2:03:04',
      );
    });

    test('clamps a negative position rather than printing one', () {
      expect(formatDuration(const Duration(seconds: -5)), '0:00');
    });
  });

  group('formatWhen', () {
    final now = DateTime(2026, 5, 26, 12, 0);

    test('spells out the recent past', () {
      expect(formatWhen(now.subtract(const Duration(seconds: 20)), now: now),
          'just now');
      expect(formatWhen(now.subtract(const Duration(minutes: 1)), now: now),
          '1 minute ago');
      expect(formatWhen(now.subtract(const Duration(minutes: 40)), now: now),
          '40 minutes ago');
      expect(formatWhen(now.subtract(const Duration(hours: 1)), now: now),
          '1 hour ago');
      expect(formatWhen(now.subtract(const Duration(days: 3)), now: now),
          '3 days ago');
    });

    test('gives the date once a week has passed', () {
      expect(
        formatWhen(now.subtract(const Duration(days: 8)), now: now),
        '2026-05-18',
      );
    });

    test('gives the date for a clock that is ahead of the server', () {
      expect(formatWhen(now.add(const Duration(hours: 2)), now: now),
          '2026-05-26');
    });
  });

  group('formatBytes', () {
    test('uses a decimal only below ten', () {
      expect(formatBytes(512), '512 B');
      expect(formatBytes(1536), '1.5 kB');
      expect(formatBytes(320 * 1024), '320 kB');
      expect(formatBytes((1.4 * 1024 * 1024).round()), '1.4 MB');
    });
  });

  group('parseServerUrl', () {
    test('assumes https when no scheme is typed', () {
      expect(parseServerUrl('iggybilly.skagedal.tech').toString(),
          'https://iggybilly.skagedal.tech');
    });

    test('keeps an explicit scheme and port', () {
      expect(parseServerUrl('http://192.168.1.4:9020').toString(),
          'http://192.168.1.4:9020');
    });

    test('reduces a pasted link to its origin', () {
      expect(
        parseServerUrl('https://iggybilly.skagedal.tech/clips/12?x=1').toString(),
        'https://iggybilly.skagedal.tech',
      );
      expect(parseServerUrl('https://example.com/').toString(),
          'https://example.com');
    });

    test('rejects what cannot be a server', () {
      expect(parseServerUrl(''), isNull);
      expect(parseServerUrl('   '), isNull);
      expect(parseServerUrl('ftp://example.com'), isNull);
      expect(parseServerUrl('https://'), isNull);
    });

    test('ignores surrounding whitespace', () {
      expect(parseServerUrl('  example.com  ').toString(), 'https://example.com');
    });
  });
}
