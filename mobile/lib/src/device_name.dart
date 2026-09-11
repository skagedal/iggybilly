import 'dart:io' show Platform;

import 'package:device_info_plus/device_info_plus.dart';

/// What to call this install in the user's list of signed-in devices.
///
/// A person with three devices needs to tell them apart to revoke the
/// right one, and "iPhone" three times over does not help. iOS gives the
/// name the owner set; Android gives the marketing name, which is the
/// closest equivalent it has.
///
/// Anything that goes wrong here falls back to a generic name rather
/// than failing a sign-in: a nameless device is a small problem, and not
/// being able to sign in is a large one.
Future<String> deviceName() async {
  try {
    final info = DeviceInfoPlugin();
    if (Platform.isIOS) {
      final ios = await info.iosInfo;
      return ios.name.isNotEmpty ? ios.name : 'iPhone';
    }
    if (Platform.isAndroid) {
      final android = await info.androidInfo;
      final model = android.model;
      final brand = android.brand;
      if (model.isEmpty) return 'Android device';
      // "Google Pixel 8" rather than "Pixel 8" when the brand isn't
      // already the first word of the model.
      if (brand.isNotEmpty && !model.toLowerCase().startsWith(brand.toLowerCase())) {
        return '$brand $model';
      }
      return model;
    }
  } catch (_) {
    // Platform channel unavailable, or a permission the OS changed its
    // mind about. A name is not worth failing over.
  }
  return 'iggybilly app';
}
