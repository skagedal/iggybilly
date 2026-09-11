import 'dart:async';

import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'common.dart';

/// Choose a label to put on a clip, or make a new one.
///
/// The server decides both what matches and whether a typed name may be
/// created — the lower-kebab-case rule lives there, and having the app
/// guess at it would mean two rules to keep in step. So this asks on
/// every keystroke, debounced, and shows exactly what it is told.
///
/// Returns the chosen name, or null if the sheet was dismissed.
Future<String?> pickLabel(BuildContext context, {required int clipId}) =>
    showModalBottomSheet<String>(
      context: context,
      isScrollControlled: true,
      builder: (_) => Padding(
        // Sit above the keyboard rather than behind it.
        padding: EdgeInsets.only(
          bottom: MediaQuery.of(context).viewInsets.bottom,
        ),
        child: _LabelPicker(clipId: clipId),
      ),
    );

class _LabelPicker extends StatefulWidget {
  const _LabelPicker({required this.clipId});

  final int clipId;

  @override
  State<_LabelPicker> createState() => _LabelPickerState();
}

class _LabelPickerState extends State<_LabelPicker> {
  final _controller = TextEditingController();
  Timer? _debounce;
  LabelSuggestions _suggestions = LabelSuggestions.empty;
  bool _loading = true;

  /// Which request is the newest. An older reply that arrives late is
  /// dropped rather than overwriting a newer one — the usual race when
  /// someone types faster than the network answers.
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _fetch('');
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _controller.dispose();
    super.dispose();
  }

  void _onChanged(String value) {
    _debounce?.cancel();
    // Long enough not to ask on every letter, short enough that the list
    // feels like it is following along.
    _debounce = Timer(const Duration(milliseconds: 180), () => _fetch(value));
  }

  Future<void> _fetch(String query) async {
    final generation = ++_generation;
    setState(() => _loading = true);
    try {
      final result = await sessionOf(context)
          .api
          .suggestLabels(query: query, clipId: widget.clipId);
      if (!mounted || generation != _generation) return;
      setState(() {
        _suggestions = result;
        _loading = false;
      });
    } on ApiException {
      if (!mounted || generation != _generation) return;
      // Leave whatever was already listed; the user can still type a
      // name and try to add it, and the add will report its own failure.
      setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _controller,
              autofocus: true,
              autocorrect: false,
              textCapitalization: TextCapitalization.none,
              decoration: InputDecoration(
                labelText: 'Label',
                hintText: 'verse-1',
                suffixIcon: _loading
                    ? const Padding(
                        padding: EdgeInsets.all(14),
                        child: SizedBox(
                          height: 16,
                          width: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        ),
                      )
                    : null,
              ),
              onChanged: _onChanged,
              onSubmitted: (value) {
                // Enter takes the create option when there is one, and
                // otherwise does nothing rather than adding something
                // the server would refuse.
                if (_suggestions.canCreate) {
                  Navigator.of(context).pop(_suggestions.query);
                }
              },
            ),
            const SizedBox(height: 8),
            if (_suggestions.canCreate)
              ListTile(
                leading: const Icon(Icons.add),
                title: Text('Create “${_suggestions.query}”'),
                onTap: () => Navigator.of(context).pop(_suggestions.query),
              ),
            if (_suggestions.matches.isEmpty && !_suggestions.canCreate)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 24),
                child: Text(
                  _controller.text.trim().isEmpty
                      ? 'No labels yet.'
                      : 'Labels are lower-case words joined by single dashes.',
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall
                      ?.copyWith(color: theme.colorScheme.outline),
                ),
              ),
            Flexible(
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final match in _suggestions.matches)
                    ListTile(
                      leading: const Icon(Icons.label_outline),
                      title: Text(match),
                      onTap: () => Navigator.of(context).pop(match),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
