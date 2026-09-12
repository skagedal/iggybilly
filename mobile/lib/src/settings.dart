import 'package:shared_preferences/shared_preferences.dart';

/// The preferences that are neither a credential nor the cache's own
/// business.
///
/// An interface for the same reason the credential store is one: platform
/// channels do not answer under `flutter test`, so anything that reached
/// for `SharedPreferences` directly would make its caller untestable.
///
/// The cache's ceiling is deliberately *not* here. It lives in the cache
/// index, next to the entries it governs, so that one object owns both
/// the policy and the files it applies to.
abstract class Settings {
  /// Whether the player loops the clip it is playing.
  Future<bool> readRepeat();
  Future<void> writeRepeat(bool value);
}

class PrefsSettings implements Settings {
  static const _repeatKey = 'iggybilly.repeat';

  @override
  Future<bool> readRepeat() async =>
      (await SharedPreferences.getInstance()).getBool(_repeatKey) ?? false;

  @override
  Future<void> writeRepeat(bool value) async =>
      (await SharedPreferences.getInstance()).setBool(_repeatKey, value);
}

/// For tests, and for a first run where the platform storage is not
/// answering.
class InMemorySettings implements Settings {
  InMemorySettings({this.repeat = false});

  bool repeat;

  @override
  Future<bool> readRepeat() async => repeat;

  @override
  Future<void> writeRepeat(bool value) async => repeat = value;
}
