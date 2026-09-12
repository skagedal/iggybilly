import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import 'common.dart';
import 'wiki_history_page.dart';

/// A label's notes: what the band decided about this verse, this take,
/// this song.
///
/// One screen with two modes rather than two screens. The source is
/// already in hand — the server sends Markdown, not HTML — so switching
/// to the editor costs nothing and does not lose your place.
///
/// Pops with `true` if anything was saved.
class LabelWikiPage extends StatefulWidget {
  const LabelWikiPage({
    super.key,
    required this.labelId,
    required this.labelName,
  });

  final int labelId;
  final String labelName;

  @override
  State<LabelWikiPage> createState() => _LabelWikiPageState();
}

class _LabelWikiPageState extends State<LabelWikiPage> {
  final _editor = TextEditingController();

  WikiPage? _page;
  String? _error;
  bool _editing = false;
  bool _saving = false;
  bool _changed = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _editor.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _error = null);
    try {
      final page = await sessionOf(context).api.wiki(widget.labelId);
      if (!mounted) return;
      setState(() {
        _page = page;
        _editor.text = page.content;
      });
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await sessionOf(context).handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    final saved = await guard(
      context,
      () => sessionOf(context).api.saveWiki(widget.labelId, _editor.text),
      whileDoing: "Couldn't save",
    );
    if (!mounted) return;
    setState(() => _saving = false);
    if (saved == null) return;

    setState(() {
      _page = saved;
      _editor.text = saved.content;
      _editing = false;
      _changed = true;
    });
  }

  /// Leaving the editor with unsaved text, on purpose or by the back
  /// gesture, should not silently throw the text away.
  Future<bool> _confirmDiscard() async {
    final page = _page;
    if (!_editing || page == null || _editor.text == page.content) return true;
    return confirm(
      context,
      title: 'Discard changes?',
      message: 'Your edits to this page have not been saved.',
      confirmLabel: 'Discard',
    );
  }

  /// Go back, asking first if there is unsaved text.
  Future<void> _leave() async {
    if (!await _confirmDiscard()) return;
    if (!mounted) return;
    Navigator.of(context).pop(_changed);
  }

  @override
  Widget build(BuildContext context) {
    final page = _page;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _leave();
      },
      child: Scaffold(
        appBar: AppBar(
          title: Text(widget.labelName),
          actions: [
            if (page != null && !_editing) ...[
              IconButton(
                tooltip: 'History',
                icon: const Icon(Icons.history),
                onPressed: () async {
                  final restored = await Navigator.of(context).push<bool>(
                    MaterialPageRoute(
                      builder: (_) => WikiHistoryPage(
                        labelId: widget.labelId,
                        labelName: widget.labelName,
                      ),
                    ),
                  );
                  if (restored == true) {
                    _changed = true;
                    await _load();
                  }
                },
              ),
              IconButton(
                tooltip: 'Edit',
                icon: const Icon(Icons.edit_outlined),
                onPressed: () => setState(() => _editing = true),
              ),
            ],
            if (_editing)
              TextButton(
                onPressed: _saving ? null : _save,
                child: _saving
                    ? const SizedBox(
                        height: 16,
                        width: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Text('Save'),
              ),
          ],
        ),
        body: _body(page),
      ),
    );
  }

  Widget _body(WikiPage? page) {
    if (page == null) {
      return _error == null
          ? const LoadingBody()
          : ErrorBody(message: _error!, onRetry: _load);
    }

    if (_editing) {
      return Padding(
        padding: const EdgeInsets.all(16),
        child: TextField(
          controller: _editor,
          autofocus: true,
          maxLines: null,
          expands: true,
          textAlignVertical: TextAlignVertical.top,
          keyboardType: TextInputType.multiline,
          decoration: const InputDecoration(
            hintText: 'Markdown. # headings, *emphasis*, - lists.',
            border: OutlineInputBorder(),
            alignLabelWithHint: true,
          ),
        ),
      );
    }

    if (!page.hasContent) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.article_outlined,
                  size: 40, color: Theme.of(context).colorScheme.outline),
              const SizedBox(height: 12),
              Text(
                'Nothing written about “${widget.labelName}” yet.',
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 16),
              FilledButton.tonal(
                onPressed: () => setState(() => _editing = true),
                child: const Text('Write something'),
              ),
            ],
          ),
        ),
      );
    }

    final theme = Theme.of(context);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 40),
        children: [
          MarkdownBody(data: page.content, selectable: true),
          if (page.lastEditedBy != null) ...[
            const SizedBox(height: 24),
            Text(
              'edited by ${page.lastEditedBy}'
              '${page.lastEditedAt == null ? '' : ' on ${formatDateTime(page.lastEditedAt!)}'}',
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
          ],
        ],
      ),
    );
  }
}
