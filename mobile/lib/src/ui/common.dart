import 'package:flutter/material.dart';

import '../api/client.dart';
import '../auth/session.dart';
import 'app_scope.dart';

/// Show a message at the bottom of the screen.
void showMessage(BuildContext context, String message, {bool isError = false}) {
  final theme = Theme.of(context);
  ScaffoldMessenger.of(context)
    ..clearSnackBars()
    ..showSnackBar(SnackBar(
      content: Text(message),
      backgroundColor: isError ? theme.colorScheme.errorContainer : null,
      showCloseIcon: isError,
    ));
}

/// Run an API call, turning its failures into something the user sees.
///
/// Returns null when the call failed, so callers read as
/// `final result = await guard(...); if (result == null) return;`.
///
/// A 401 is handled rather than shown: the token has been revoked, so
/// the session is dropped and the app returns to the sign-in screen.
/// There is nothing a user can do with the words "invalid or revoked
/// token" on a screen they can no longer use.
Future<T?> guard<T>(
  BuildContext context,
  Future<T> Function() call, {
  String? whileDoing,
}) async {
  final session = AppScope.of(context).session;
  try {
    return await call();
  } on ApiException catch (e) {
    if (e.isUnauthorized) {
      await session.handleUnauthorized();
      return null;
    }
    if (context.mounted) {
      showMessage(
        context,
        whileDoing == null ? e.message : '$whileDoing: ${e.message}',
        isError: true,
      );
    }
    return null;
  }
}

/// [guard], for a call that returns nothing. Reports whether it worked,
/// since `await guard(...)` on a `Future<void>` yields a value there is
/// no legal way to test.
Future<bool> guardDone(
  BuildContext context,
  Future<void> Function() call, {
  String? whileDoing,
}) async {
  final done = await guard(context, () async {
    await call();
    return true;
  }, whileDoing: whileDoing);
  return done ?? false;
}

/// The middle of a screen while its first load is in flight.
class LoadingBody extends StatelessWidget {
  const LoadingBody({super.key});

  @override
  Widget build(BuildContext context) =>
      const Center(child: CircularProgressIndicator());
}

/// A screen that could not load, with the reason and a way to try again.
class ErrorBody extends StatelessWidget {
  const ErrorBody({super.key, required this.message, required this.onRetry});

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.cloud_off,
                  size: 40, color: Theme.of(context).colorScheme.outline),
              const SizedBox(height: 12),
              Text(message, textAlign: TextAlign.center),
              const SizedBox(height: 16),
              FilledButton.tonal(onPressed: onRetry, child: const Text('Try again')),
            ],
          ),
        ),
      );
}

/// A screen with nothing on it yet, and a line saying why that is fine.
class EmptyBody extends StatelessWidget {
  const EmptyBody({super.key, required this.icon, required this.message});

  final IconData icon;
  final String message;

  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 40, color: Theme.of(context).colorScheme.outline),
              const SizedBox(height: 12),
              Text(
                message,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
            ],
          ),
        ),
      );
}

/// Ask before doing something that cannot be undone.
Future<bool> confirm(
  BuildContext context, {
  required String title,
  required String message,
  required String confirmLabel,
  bool destructive = true,
}) async {
  final answer = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: Text(message),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: const Text('Cancel'),
        ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(true),
          style: destructive
              ? TextButton.styleFrom(
                  foregroundColor: Theme.of(context).colorScheme.error)
              : null,
          child: Text(confirmLabel),
        ),
      ],
    ),
  );
  return answer ?? false;
}

/// The session, without listening to it. Screens that need to rebuild on
/// a change use a ListenableBuilder instead.
Session sessionOf(BuildContext context) => AppScope.of(context).session;
