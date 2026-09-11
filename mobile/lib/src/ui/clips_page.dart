import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../format.dart';
import '../player/player_controller.dart';
import 'account_page.dart';
import 'app_scope.dart';
import 'clip_page.dart';
import 'common.dart';
import 'upload.dart';
import 'waveform.dart';
import 'wiki_page.dart';

/// The clip list, filtered by label.
///
/// The filter is AND, as on the web: adding a label narrows rather than
/// widens. Tapping a label on a row adds it to the filter, which is the
/// quickest way to get from "here is a clip" to "here is everything like
/// it".
class ClipsPage extends StatefulWidget {
  const ClipsPage({super.key, this.initialFilters = const []});

  final List<String> initialFilters;

  @override
  State<ClipsPage> createState() => _ClipsPageState();
}

class _ClipsPageState extends State<ClipsPage> {
  late List<String> _filters = List.of(widget.initialFilters);

  List<Clip>? _clips;
  List<WikiPage> _wikis = const [];
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    // Not awaited: the first frame is the spinner.
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    final api = sessionOf(context).api;
    try {
      final clips = await api.clips(labels: _filters);
      // The wiki page for each active filter, shown above the clips —
      // a filtered list is a subject, and the subject's notes belong
      // with it. Resolved from the clips we already have, so a filter
      // for a label nobody has written about costs no request.
      final ids = <int, String>{};
      for (final clip in clips) {
        for (final label in clip.labels) {
          if (_filters.any((f) => f.toLowerCase() == label.name.toLowerCase())) {
            ids[label.id] = label.name;
          }
        }
      }
      final wikis = <WikiPage>[];
      for (final id in ids.keys) {
        final page = await api.wiki(id);
        if (page.hasContent) wikis.add(page);
      }

      if (!mounted) return;
      setState(() {
        _clips = clips;
        _wikis = wikis;
        _loading = false;
      });
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await sessionOf(context).handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() {
        _error = e.message;
        _loading = false;
      });
    }
  }

  void _addFilter(String name) {
    if (_filters.any((f) => f.toLowerCase() == name.toLowerCase())) return;
    setState(() => _filters = [..._filters, name]);
    _load();
  }

  void _removeFilter(String name) {
    setState(() => _filters = _filters.where((f) => f != name).toList());
    _load();
  }

  Future<void> _openClip(Clip clip) async {
    final changed = await Navigator.of(context).push<bool>(
      MaterialPageRoute(builder: (_) => ClipPage(clipId: clip.id)),
    );
    // The clip page can rename, relabel or delete; rather than trying to
    // reconcile that here, reload when it says something changed.
    if (changed == true) await _load();
  }

  Future<void> _upload() async {
    final uploaded = await pickAndUpload(context);
    if (uploaded > 0) await _load();
  }

  @override
  Widget build(BuildContext context) {
    final player = AppScope.of(context).player;

    return Scaffold(
      appBar: AppBar(
        title: const Text('iggybilly'),
        actions: [
          IconButton(
            tooltip: 'Account',
            icon: const Icon(Icons.person_outline),
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute(builder: (_) => const AccountPage()),
            ),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _upload,
        tooltip: 'Upload clips',
        child: const Icon(Icons.add),
      ),
      body: RefreshIndicator(
        onRefresh: _load,
        child: _body(player),
      ),
    );
  }

  Widget _body(PlayerController player) {
    if (_loading && _clips == null) return const LoadingBody();
    if (_error != null && _clips == null) {
      return ErrorBody(message: _error!, onRetry: _load);
    }

    final clips = _clips ?? const <Clip>[];
    return CustomScrollView(
      // Always scrollable, so pull-to-refresh works on a short list.
      physics: const AlwaysScrollableScrollPhysics(),
      slivers: [
        if (_filters.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
              child: Wrap(
                spacing: 6,
                runSpacing: 6,
                children: [
                  for (final filter in _filters)
                    InputChip(
                      label: Text(filter),
                      onDeleted: () => _removeFilter(filter),
                    ),
                ],
              ),
            ),
          ),
        for (final wiki in _wikis)
          SliverToBoxAdapter(child: _WikiCard(page: wiki, onChanged: _load)),
        if (clips.isEmpty)
          SliverFillRemaining(
            hasScrollBody: false,
            child: EmptyBody(
              icon: Icons.graphic_eq,
              message: _filters.isEmpty
                  ? 'No clips yet.\nTap + to upload one.'
                  : 'No clips carry all of those labels.',
            ),
          )
        else
          SliverList.separated(
            itemCount: clips.length,
            separatorBuilder: (_, _) => const Divider(height: 1),
            itemBuilder: (context, i) => _ClipRow(
              clip: clips[i],
              player: player,
              activeFilters: _filters,
              onOpen: () => _openClip(clips[i]),
              onLabelTapped: _addFilter,
            ),
          ),
        // Room to scroll the last row clear of the floating button.
        const SliverToBoxAdapter(child: SizedBox(height: 88)),
      ],
    );
  }
}

class _ClipRow extends StatelessWidget {
  const _ClipRow({
    required this.clip,
    required this.player,
    required this.activeFilters,
    required this.onOpen,
    required this.onLabelTapped,
  });

  final Clip clip;
  final PlayerController player;
  final List<String> activeFilters;
  final VoidCallback onOpen;
  final ValueChanged<String> onLabelTapped;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final session = sessionOf(context);

    return ListenableBuilder(
      listenable: player,
      builder: (context, _) {
        final isLoaded = player.clip?.id == clip.id;
        final isPlaying = isLoaded && player.isPlaying;

        return InkWell(
          onTap: onOpen,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(4, 8, 12, 8),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                IconButton(
                  tooltip: isPlaying ? 'Pause' : 'Play',
                  icon: Icon(isPlaying ? Icons.pause_circle : Icons.play_circle,
                      size: 34),
                  color: isLoaded ? theme.colorScheme.primary : null,
                  onPressed: () => player.play(
                    clip,
                    session.api.resolve(clip.audioPath),
                    headers: session.api.authHeaders,
                  ),
                ),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        clip.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleSmall,
                      ),
                      const SizedBox(height: 4),
                      Waveform(
                        peaks: clip.peaks,
                        // Only the loaded clip shows a position; the
                        // rest are pictures of a waveform.
                        progress: isLoaded ? player.progress : 0,
                        height: 30,
                      ),
                      const SizedBox(height: 4),
                      Text(
                        _subtitle(),
                        style: theme.textTheme.bodySmall
                            ?.copyWith(color: theme.colorScheme.outline),
                      ),
                      if (clip.labels.isNotEmpty) ...[
                        const SizedBox(height: 6),
                        Wrap(
                          spacing: 4,
                          runSpacing: 4,
                          children: [
                            for (final label in clip.labels)
                              _LabelChip(
                                name: label.name,
                                active: activeFilters.any((f) =>
                                    f.toLowerCase() == label.name.toLowerCase()),
                                onTap: () => onLabelTapped(label.name),
                              ),
                          ],
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }

  String _subtitle() {
    final parts = <String>[clip.uploader, formatWhen(clip.uploadedAt)];
    // The recording date is what a band actually cares about, so it is
    // named rather than left to be confused with the upload time.
    if (clip.recordingDate != null) parts.add('rec. ${clip.recordingDate}');
    if (clip.duration != null) parts.add(formatDuration(clip.duration!));
    return parts.join(' · ');
  }
}

class _LabelChip extends StatelessWidget {
  const _LabelChip({required this.name, required this.active, required this.onTap});

  final String name;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(20),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
        decoration: BoxDecoration(
          color: active
              ? theme.colorScheme.primaryContainer
              : theme.colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(20),
        ),
        child: Text(name, style: theme.textTheme.labelSmall),
      ),
    );
  }
}

/// A filtered label's wiki page, above the clips it describes.
class _WikiCard extends StatelessWidget {
  const _WikiCard({required this.page, required this.onChanged});

  final WikiPage page;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      margin: const EdgeInsets.fromLTRB(12, 10, 12, 2),
      child: InkWell(
        onTap: () async {
          final changed = await Navigator.of(context).push<bool>(
            MaterialPageRoute(
              builder: (_) => LabelWikiPage(
                labelId: page.labelId,
                labelName: page.labelName,
              ),
            ),
          );
          if (changed == true) onChanged();
        },
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(Icons.article_outlined,
                      size: 16, color: theme.colorScheme.primary),
                  const SizedBox(width: 6),
                  Text(page.labelName, style: theme.textTheme.titleSmall),
                ],
              ),
              const SizedBox(height: 6),
              // A preview, not the page: the wiki screen is a tap away,
              // and a long page would push the clips off the screen.
              Text(
                page.content,
                maxLines: 4,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
