import 'package:flutter/material.dart';

import 'src/auth/credential_store.dart';
import 'src/auth/session.dart';
import 'src/device_name.dart';
import 'src/ui/app.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(
    IggybillyApp(
      session: Session(
        store: SecureCredentialStore(),
        deviceName: deviceName,
      ),
    ),
  );
}
