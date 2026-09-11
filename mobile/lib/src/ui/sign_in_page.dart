import 'package:flutter/material.dart';

import '../api/client.dart';
import '../format.dart';
import 'app_scope.dart';

/// Signing in, and choosing which server to sign in to.
///
/// The server field is here rather than hidden in settings because this
/// app is for self-hosted instances: the address is part of the
/// credential, and someone installing it for their own band needs to
/// type theirs before anything else can work.
class SignInPage extends StatefulWidget {
  const SignInPage({super.key});

  @override
  State<SignInPage> createState() => _SignInPageState();
}

class _SignInPageState extends State<SignInPage> {
  final _formKey = GlobalKey<FormState>();
  final _server = TextEditingController();
  final _username = TextEditingController();
  final _password = TextEditingController();

  bool _busy = false;
  bool _showPassword = false;
  bool _prefilled = false;
  String? _error;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Here rather than initState: reading an InheritedWidget is only
    // legal once dependencies are available. Guarded so a rebuild does
    // not overwrite what the user has started typing.
    if (_prefilled) return;
    _prefilled = true;
    final session = AppScope.of(context).session;
    // Prefilled from the last sign-in: this is a band's own server, not
    // one of many, so retyping it every time would be silly.
    _server.text = session.server.toString();
    _username.text = session.lastUsername ?? '';
  }

  @override
  void dispose() {
    _server.dispose();
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (!_formKey.currentState!.validate()) return;
    final server = parseServerUrl(_server.text);
    if (server == null) return;

    setState(() {
      _busy = true;
      _error = null;
    });

    try {
      await AppScope.of(context).session.signIn(
            server: server,
            username: _username.text.trim(),
            password: _password.text,
          );
      // On success this widget is replaced by the signed-in tree, so
      // there is nothing to set state on.
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
    final theme = Theme.of(context);
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: Form(
                key: _formKey,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Icon(Icons.graphic_eq,
                        size: 56, color: theme.colorScheme.primary),
                    const SizedBox(height: 12),
                    Text(
                      'iggybilly',
                      textAlign: TextAlign.center,
                      style: theme.textTheme.headlineSmall,
                    ),
                    const SizedBox(height: 28),
                    TextFormField(
                      controller: _server,
                      decoration: const InputDecoration(
                        labelText: 'Server',
                        hintText: 'iggybilly.example.com',
                        prefixIcon: Icon(Icons.dns_outlined),
                      ),
                      keyboardType: TextInputType.url,
                      autocorrect: false,
                      enabled: !_busy,
                      validator: (v) => parseServerUrl(v ?? '') == null
                          ? "That doesn't look like a server address."
                          : null,
                    ),
                    const SizedBox(height: 12),
                    TextFormField(
                      controller: _username,
                      decoration: const InputDecoration(
                        labelText: 'Username',
                        prefixIcon: Icon(Icons.person_outline),
                      ),
                      autocorrect: false,
                      enabled: !_busy,
                      textInputAction: TextInputAction.next,
                      validator: (v) => (v ?? '').trim().isEmpty
                          ? 'Enter your username.'
                          : null,
                    ),
                    const SizedBox(height: 12),
                    TextFormField(
                      controller: _password,
                      decoration: InputDecoration(
                        labelText: 'Password',
                        prefixIcon: const Icon(Icons.lock_outline),
                        suffixIcon: IconButton(
                          onPressed: () =>
                              setState(() => _showPassword = !_showPassword),
                          icon: Icon(_showPassword
                              ? Icons.visibility_off_outlined
                              : Icons.visibility_outlined),
                          tooltip: _showPassword ? 'Hide' : 'Show',
                        ),
                      ),
                      obscureText: !_showPassword,
                      enabled: !_busy,
                      textInputAction: TextInputAction.done,
                      onFieldSubmitted: (_) => _busy ? null : _submit(),
                      validator: (v) =>
                          (v ?? '').isEmpty ? 'Enter your password.' : null,
                    ),
                    if (_error != null) ...[
                      const SizedBox(height: 16),
                      Text(
                        _error!,
                        style: theme.textTheme.bodyMedium
                            ?.copyWith(color: theme.colorScheme.error),
                      ),
                    ],
                    const SizedBox(height: 24),
                    FilledButton(
                      onPressed: _busy ? null : _submit,
                      child: _busy
                          ? const SizedBox(
                              height: 18,
                              width: 18,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Text('Sign in'),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
