import 'package:material_ui/material_ui.dart';

import 'account_page.dart';
import 'common.dart';

/// What the app shows an account that is in no band.
///
/// Signing in worked — the password was right — and there is simply
/// nowhere to go: a new account before the site owner has assigned it a
/// band, or Apple's reviewer before the test band is ready. So this says
/// what is wrong and who can fix it, and leaves the way to the account
/// screen open so the one thing that still works — signing out — is
/// still reachable.
///
/// It is deliberately not an error, and deliberately not an empty clip
/// list: an empty list says "this band has no clips yet", which is a
/// different and much more confusing thing to be told.
class NoBandPage extends StatelessWidget {
  const NoBandPage({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final session = sessionOf(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('iggybilly'),
        actions: [
          IconButton(
            tooltip: 'Account',
            icon: const Icon(Icons.person_outline),
            onPressed: () => Navigator.of(context)
                .push(MaterialPageRoute(builder: (_) => const AccountPage())),
          ),
        ],
      ),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(
                Icons.group_outlined,
                size: 48,
                color: theme.colorScheme.outline,
              ),
              const SizedBox(height: 16),
              Text(
                'You are not in a band yet',
                style: theme.textTheme.titleMedium,
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 8),
              Text(
                'Signed in as ${session.user?.username ?? '—'}. Your account '
                'works, but it has not been added to a band, so there are no '
                'clips to show. Ask whoever runs this iggybilly to add you '
                'to one.',
                style: theme.textTheme.bodyMedium,
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
