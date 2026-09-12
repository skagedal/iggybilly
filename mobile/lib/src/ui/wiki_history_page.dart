import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import 'common.dart';

/// Every revision of a label's notes, newest first.
///
/// Restoring appends rather than rewinds — the server writes the old
/// text as a new revision — so nothing between then and now is lost, and
/// the screen says so rather than making it look destructive.
///
/// Pops with `true` if a revision was restored.
class WikiHistoryPage extends StatefulWidget {
  const WikiHistoryPage({
    super.key,
    required this.labelId,
    required this.labelName,
  });

  final int labelId;
  final String labelName;

  @override
  State<WikiHistoryPage> createState() => _WikiHistoryPageState();
}

class _WikiHistoryPageState extends State<WikiHistoryPage> {
  List<WikiRevision>? _revisions;
  String? _error;
  bool _restored = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _error = null);
    try {
      final revisions = await sessionOf(context).api.wikiHistory(widget.labelId);
      if (!mounted) return;
      setState(() => _revisions = revisions);
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await sessionOf(context).handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _restore(WikiRevision revision) async {
    final sure = await confirm(
      context,
      title: 'Restore this version?',
      message: 'It is added as a new revision, so the current text is '
          'kept in the history too.',
      confirmLabel: 'Restore',
      destructive: false,
    );
    if (!sure || !mounted) return;

    final done = await guardDone(
      context,
      () => sessionOf(context).api.restoreWiki(widget.labelId, revision.id),
    );
    if (!done || !mounted) return;
    _restored = true;
    await _load();
    if (mounted) showMessage(context, 'Restored.');
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) Navigator.of(context).pop(_restored);
      },
      child: Scaffold(
        appBar: AppBar(title: Text('${widget.labelName} — history')),
        body: _body(),
      ),
    );
  }

  Widget _body() {
    final revisions = _revisions;
    if (revisions == null) {
      return _error == null
          ? const LoadingBody()
          : ErrorBody(message: _error!, onRetry: _load);
    }
    if (revisions.isEmpty) {
      return const EmptyBody(
        icon: Icons.history,
        message: 'This page has never been edited.',
      );
    }

    final theme = Theme.of(context);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView.separated(
        physics: const AlwaysScrollableScrollPhysics(),
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 40),
        itemCount: revisions.length,
        separatorBuilder: (_, _) => const Divider(height: 24),
        itemBuilder: (context, i) {
          final revision = revisions[i];
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      '${revision.author} · ${formatDateTime(revision.editedAt)}',
                      style: theme.textTheme.labelMedium,
                    ),
                  ),
                  if (revision.isCurrent)
                    Chip(
                      label: const Text('Current'),
                      visualDensity: VisualDensity.compact,
                      labelStyle: theme.textTheme.labelSmall,
                    )
                  else
                    TextButton(
                      onPressed: () => _restore(revision),
                      child: const Text('Restore'),
                    ),
                ],
              ),
              const SizedBox(height: 4),
              if (revision.content.trim().isEmpty)
                Text(
                  '(empty)',
                  style: theme.textTheme.bodySmall
                      ?.copyWith(color: theme.colorScheme.outline),
                )
              else
                MarkdownBody(data: revision.content),
            ],
          );
        },
      ),
    );
  }
}
