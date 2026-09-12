import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import 'src/auth/credential_store.dart';
import 'src/auth/session.dart';
import 'src/cache/track_cache.dart';
import 'src/device_name.dart';
import 'src/settings.dart';
import 'src/ui/app.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(
    IggybillyApp(
      session: Session(
        store: SecureCredentialStore(),
        deviceName: deviceName,
      ),
      // The directory arrives as a function rather than a path, because
      // asking the platform for it is asynchronous and the first frame
      // should not wait on a disk cache.
      cache: TrackCache(locate: _cacheDirectory),
      settings: PrefsSettings(),
    ),
  );
}

/// Where clips are kept on disk.
///
/// Its own subdirectory of the app support directory, because the cache
/// deletes any file in there that its index does not account for — and
/// that is only a safe rule to have in a directory nothing else writes
/// to.
Future<Directory> _cacheDirectory() async {
  final support = await getApplicationSupportDirectory();
  return Directory(p.join(support.path, 'clips'));
}
