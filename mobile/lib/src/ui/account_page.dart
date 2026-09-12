import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import 'app_scope.dart';
import 'common.dart';
import 'storage_page.dart';

/// The account: who you are, which devices are signed in, and the way
/// out.
///
/// The device list is the reason token auth was worth the trouble. A
/// phone left in a taxi is one row here and one tap, rather than a
/// password change and every other device signed out with it.
class AccountPage extends StatefulWidget {
  const AccountPage({super.key});

  @override
  State<AccountPage> createState() => _AccountPageState();
}

class _AccountPageState extends State<AccountPage> {
  List<Device>? _devices;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _error = null);
    try {
      final devices = await sessionOf(context).api.devices();
      if (!mounted) return;
      setState(() => _devices = devices);
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await sessionOf(context).handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _revoke(Device device) async {
    final sure = await confirm(
      context,
      title: 'Sign out “${device.name}”?',
      message: 'That device will have to sign in again.',
      confirmLabel: 'Sign out',
    );
    if (!sure || !mounted) return;

    final done =
        await guardDone(context, () => sessionOf(context).api.revokeDevice(device.id));
    if (!done || !mounted) return;
    await _load();
  }

  Future<void> _changePassword() async {
    final changed = await showDialog<bool>(
      context: context,
      builder: (_) => const _ChangePasswordDialog(),
    );
    if (changed == true && mounted) {
      // Every other device was signed out by the change.
      await _load();
      if (mounted) showMessage(context, 'Password changed.');
    }
  }

  Future<void> _signOut() async {
    final sure = await confirm(
      context,
      title: 'Sign out?',
      message: 'This device will need your password again.',
      confirmLabel: 'Sign out',
    );
    if (!sure || !mounted) return;
    await sessionOf(context).signOut();
    // The signed-out tree replaces everything above; nothing to pop.
  }

  @override
  Widget build(BuildContext context) {
    final session = sessionOf(context);
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(title: const Text('Account')),
      body: RefreshIndicator(
        onRefresh: _load,
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: const EdgeInsets.only(bottom: 40),
          children: [
            ListTile(
              leading: const Icon(Icons.person_outline),
              title: Text(session.user?.username ?? '—'),
              subtitle: Text(session.server.toString()),
            ),
            const Divider(),
            if (AppScope.of(context).cache case final cache?)
              ListTile(
                leading: const Icon(Icons.download_outlined),
                title: const Text('Downloads'),
                subtitle: const Text(
                  'How much of the band is kept on this phone',
                ),
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(builder: (_) => StoragePage(cache: cache)),
                ),
              ),
            ListTile(
              leading: const Icon(Icons.lock_outline),
              title: const Text('Change password'),
              subtitle: const Text('Signs every other device out'),
              onTap: _changePassword,
            ),
            const Divider(),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
              child: Text('Signed-in devices', style: theme.textTheme.titleSmall),
            ),
            ..._deviceRows(theme),
            const Divider(),
            ListTile(
              leading: Icon(Icons.logout, color: theme.colorScheme.error),
              title: Text(
                'Sign out',
                style: TextStyle(color: theme.colorScheme.error),
              ),
              onTap: _signOut,
            ),
          ],
        ),
      ),
    );
  }

  List<Widget> _deviceRows(ThemeData theme) {
    final devices = _devices;
    if (devices == null) {
      return [
        if (_error != null)
          Padding(
            padding: const EdgeInsets.all(16),
            child: Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
          )
        else
          const Padding(
            padding: EdgeInsets.all(24),
            child: Center(child: CircularProgressIndicator()),
          ),
      ];
    }

    return [
      for (final device in devices)
        ListTile(
          leading: Icon(
            device.isCurrent ? Icons.smartphone : Icons.devices_other,
            color: device.isCurrent ? theme.colorScheme.primary : null,
          ),
          title: Text(device.name + (device.isCurrent ? ' (this device)' : '')),
          subtitle: Text(
            device.lastUsedOn == null
                ? 'Added ${formatDate(device.createdAt)}, not used since'
                : 'Added ${formatDate(device.createdAt)} · last used ${device.lastUsedOn}',
          ),
          trailing: device.isCurrent
              ? null
              : IconButton(
                  tooltip: 'Sign this device out',
                  icon: const Icon(Icons.close),
                  onPressed: () => _revoke(device),
                ),
        ),
    ];
  }
}

class _ChangePasswordDialog extends StatefulWidget {
  const _ChangePasswordDialog();

  @override
  State<_ChangePasswordDialog> createState() => _ChangePasswordDialogState();
}

class _ChangePasswordDialogState extends State<_ChangePasswordDialog> {
  final _formKey = GlobalKey<FormState>();
  final _current = TextEditingController();
  final _next = TextEditingController();
  final _confirm = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _current.dispose();
    _next.dispose();
    _confirm.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (!_formKey.currentState!.validate()) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await sessionOf(context).changePassword(
        currentPassword: _current.text,
        newPassword: _next.text,
      );
      if (mounted) Navigator.of(context).pop(true);
    } on ApiException catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = e.message;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Change password'),
      content: Form(
        key: _formKey,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextFormField(
              controller: _current,
              obscureText: true,
              enabled: !_busy,
              decoration: const InputDecoration(labelText: 'Current password'),
              validator: (v) =>
                  (v ?? '').isEmpty ? 'Enter your current password.' : null,
            ),
            const SizedBox(height: 12),
            TextFormField(
              controller: _next,
              obscureText: true,
              enabled: !_busy,
              decoration: const InputDecoration(labelText: 'New password'),
              // The same floor the server enforces, checked here so the
              // answer is immediate rather than a round trip.
              validator: (v) => (v ?? '').length < 10
                  ? 'At least 10 characters.'
                  : null,
            ),
            const SizedBox(height: 12),
            TextFormField(
              controller: _confirm,
              obscureText: true,
              enabled: !_busy,
              decoration: const InputDecoration(labelText: 'Repeat new password'),
              // Never sent: matching the two is a typo guard, and the
              // server has no use for the second copy.
              validator: (v) =>
                  v != _next.text ? "Those don't match." : null,
            ),
            if (_error != null) ...[
              const SizedBox(height: 12),
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: _busy ? null : () => Navigator.of(context).pop(false),
          child: const Text('Cancel'),
        ),
        TextButton(
          onPressed: _busy ? null : _submit,
          child: const Text('Change'),
        ),
      ],
    );
  }
}
