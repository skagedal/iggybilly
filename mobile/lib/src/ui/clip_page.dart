import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import 'app_scope.dart';
import 'clip_format.dart';
import 'common.dart';
import 'keep_downloaded.dart';
import 'label_picker.dart';
import 'waveform.dart';
import 'wiki_page.dart';

/// One clip: play it, share its file, rename it, label it, read its
/// labels' notes, and — if you uploaded it — delete it.
///
/// Pops with `true` when something changed, so the list behind it
/// reloads rather than showing a name or a label that is no longer
/// right.
class ClipPage extends StatefulWidget {
  const ClipPage({super.key, required this.clipId});

  final int clipId;

  @override
  State<ClipPage> createState() => _ClipPageState();
}

class _ClipPageState extends State<ClipPage> {
  Clip? _clip;
  String? _error;
  bool _changed = false;
  bool _sharing = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _error = null);
    try {
      final clip = await sessionOf(context).api.clip(widget.clipId);
      if (!mounted) return;
      setState(() => _clip = clip);
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await sessionOf(context).handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() => _error = e.message);
    }
  }

  Future<void> _rename() async {
    final clip = _clip!;
    final name = await showDialog<String>(
      context: context,
      builder: (_) => _RenameDialog(initial: clip.name),
    );
    if (name == null || name == clip.name || !mounted) return;

    final settled = await guard(context, () => sessionOf(context).api.renameClip(clip.id, name));
    if (settled == null || !mounted) return;

    setState(() {
      _clip = clip.withName(settled);
      _changed = true;
    });
    // The bar is showing the old name if this is the loaded clip.
    AppScope.of(context).player.renamed(clip.id, settled);
  }

  /// Fetch the file as uploaded and hand it to the system share sheet,
  /// from where it can be saved to Files or opened in another app.
  ///
  /// The copy is left in place when the sheet closes, because on Android
  /// the receiving app may read it after that, and cleared at the start
  /// of the next share instead.
  Future<void> _share(Rect? origin) async {
    final clip = _clip!;
    setState(() => _sharing = true);
    try {
      final bytes = await guard(
        context,
        () => sessionOf(context).api.download(clip.downloadPath),
        whileDoing: "Couldn't fetch the file",
      );
      if (bytes == null || !mounted) return;

      final dir = Directory(
        p.join((await getTemporaryDirectory()).path, 'shared'),
      );
      if (await dir.exists()) await dir.delete(recursive: true);
      await dir.create(recursive: true);
      final name = p.basename(clip.originalFilename);
      final file = File(p.join(dir.path, name.isEmpty ? 'clip' : name));
      await file.writeAsBytes(bytes);

      await SharePlus.instance.share(
        ShareParams(files: [XFile(file.path)], sharePositionOrigin: origin),
      );
    } finally {
      if (mounted) setState(() => _sharing = false);
    }
  }

  Future<void> _addLabel() async {
    final name = await pickLabel(context, clipId: widget.clipId);
    if (name == null || !mounted) return;

    final labels = await guard(
      context,
      () => sessionOf(context).api.addLabel(widget.clipId, name),
    );
    if (labels == null || !mounted) return;
    setState(() {
      _clip = _clip!.withLabels(labels);
      _changed = true;
    });
  }

  Future<void> _removeLabel(Label label) async {
    final labels = await guard(
      context,
      () => sessionOf(context).api.removeLabel(widget.clipId, label.id),
    );
    if (labels == null || !mounted) return;
    setState(() {
      _clip = _clip!.withLabels(labels);
      _changed = true;
    });
  }

  Future<void> _delete() async {
    final clip = _clip!;
    final sure = await confirm(
      context,
      title: 'Delete “${clip.name}”?',
      message: 'The clip and its audio file are removed for everyone. '
          'This cannot be undone.',
      confirmLabel: 'Delete',
    );
    if (!sure || !mounted) return;

    final player = AppScope.of(context).player;
    final done = await guardDone(context, () => sessionOf(context).api.deleteClip(clip.id));
    if (!done) return;

    // Stop first: a bar playing a clip that no longer exists is a
    // confusing thing to leave on screen.
    if (player.clip?.id == clip.id) await player.stop();
    if (!mounted) return;
    Navigator.of(context).pop(true);
  }

  Future<void> _openWiki(Label label) async {
    final changed = await Navigator.of(context).push<bool>(
      MaterialPageRoute(
        builder: (_) => LabelWikiPage(labelId: label.id, labelName: label.name),
      ),
    );
    if (changed == true) _changed = true;
  }

  @override
  Widget build(BuildContext context) {
    final clip = _clip;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) Navigator.of(context).pop(_changed);
      },
      child: Scaffold(
        appBar: AppBar(
          title: Row(
            children: [
              Flexible(child: Text(clip?.name ?? 'Clip')),
              if (clip != null && clip.isMidi) ...[
                const SizedBox(width: 8),
                FormatBadge(clip: clip),
              ],
            ],
          ),
          actions: [
            if (clip != null)
              Builder(
                builder: (context) => IconButton(
                  tooltip: 'Share file',
                  icon: _sharing
                      ? const SizedBox.square(
                          dimension: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : Icon(Icons.adaptive.share),
                  onPressed: _sharing
                      ? null
                      : () {
                          // iPads anchor the share popover to the button.
                          final box = context.findRenderObject() as RenderBox?;
                          _share(
                            box == null
                                ? null
                                : box.localToGlobal(Offset.zero) & box.size,
                          );
                        },
                ),
              ),
            if (clip != null)
              IconButton(
                tooltip: 'Rename',
                icon: const Icon(Icons.edit_outlined),
                onPressed: _rename,
              ),
            if (clip != null && clip.canDelete)
              IconButton(
                tooltip: 'Delete',
                icon: const Icon(Icons.delete_outline),
                onPressed: _delete,
              ),
          ],
        ),
        body: _body(clip),
      ),
    );
  }

  Widget _body(Clip? clip) {
    if (clip == null) {
      return _error == null
          ? const LoadingBody()
          : ErrorBody(message: _error!, onRetry: _load);
    }

    final theme = Theme.of(context);
    final player = AppScope.of(context).player;
    final session = sessionOf(context);

    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 40),
        children: [
          if (!clip.playable)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 24),
              child: Text(
                unplayableMessage,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.outline,
                ),
              ),
            )
          else
            ListenableBuilder(
              listenable: player,
              builder: (context, _) {
                final isLoaded = player.clip?.id == clip.id;
                final isPlaying = isLoaded && player.isPlaying;
                return Column(
                  children: [
                    Waveform(
                      peaks: clip.peaks,
                      progress: isLoaded ? player.progress : 0,
                      height: 72,
                      // Seeking only makes sense on the clip that is
                      // actually loaded.
                      onSeek: isLoaded ? player.seekFraction : null,
                    ),
                    const SizedBox(height: 8),
                    Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        IconButton.filled(
                          iconSize: 34,
                          icon: Icon(
                            isPlaying ? Icons.pause : Icons.play_arrow,
                          ),
                          tooltip: isPlaying ? 'Pause' : 'Play',
                          onPressed: () => player.play(
                            clip,
                            session.api.resolve(clip.audioPath),
                            headers: session.api.authHeaders,
                          ),
                        ),
                        const SizedBox(width: 16),
                        Text(
                          isLoaded && player.duration != null
                              ? '${formatDuration(player.position)} / ${formatDuration(player.duration!)}'
                              : clip.duration == null
                              ? '—'
                              : formatDuration(clip.duration!),
                          style: theme.textTheme.titleMedium,
                        ),
                      ],
                    ),
                  ],
                );
              },
            ),
          if (clip.playable && clip.isMidi) ...[
            const SizedBox(height: 8),
            Text(
              renderingNote,
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
          ],
          if (clip.playable) ...[
            const SizedBox(height: 16),
            KeepDownloadedTile(clip: clip),
          ],
          const SizedBox(height: 16),
          _Facts(clip: clip),
          const SizedBox(height: 24),
          Row(
            children: [
              Text('Labels', style: theme.textTheme.titleSmall),
              const Spacer(),
              TextButton.icon(
                onPressed: _addLabel,
                icon: const Icon(Icons.add, size: 18),
                label: const Text('Add'),
              ),
            ],
          ),
          if (clip.labels.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Text(
                'No labels yet. Labels are how you find this again.',
                style: theme.textTheme.bodySmall
                    ?.copyWith(color: theme.colorScheme.outline),
              ),
            )
          else
            Wrap(
              spacing: 8,
              runSpacing: 4,
              children: [
                for (final label in clip.labels)
                  InputChip(
                    label: Text(label.name),
                    // The chip opens the label's notes; the X takes it
                    // off the clip.
                    onPressed: () => _openWiki(label),
                    onDeleted: () => _removeLabel(label),
                    deleteButtonTooltipMessage: 'Remove from this clip',
                  ),
              ],
            ),
        ],
      ),
    );
  }
}

class _Facts extends StatelessWidget {
  const _Facts({required this.clip});

  final Clip clip;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final rows = <(String, String)>[
      ('Uploaded by', clip.uploader),
      ('Uploaded', formatDateTime(clip.uploadedAt)),
      if (clip.recordingDate != null) ('Recorded', clip.recordingDate!),
      ('File', clip.originalFilename),
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final (label, value) in rows)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 3),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: 110,
                  child: Text(
                    label,
                    style: theme.textTheme.bodySmall
                        ?.copyWith(color: theme.colorScheme.outline),
                  ),
                ),
                Expanded(child: Text(value, style: theme.textTheme.bodyMedium)),
              ],
            ),
          ),
      ],
    );
  }
}

class _RenameDialog extends StatefulWidget {
  const _RenameDialog({required this.initial});

  final String initial;

  @override
  State<_RenameDialog> createState() => _RenameDialogState();
}

class _RenameDialogState extends State<_RenameDialog> {
  late final _controller = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final name = _controller.text.trim();
    if (name.isEmpty) return;
    Navigator.of(context).pop(name);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
        title: const Text('Rename clip'),
        content: TextField(
          controller: _controller,
          autofocus: true,
          decoration: const InputDecoration(labelText: 'Name'),
          onSubmitted: (_) => _submit(),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          TextButton(onPressed: _submit, child: const Text('Rename')),
        ],
      );
}
