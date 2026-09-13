import 'dart:async';

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
///
/// With exactly one label the list is that label's playlist: in the
/// band's order, reorderable by a long press on a row's handle, and
/// played as a queue. Several labels have no order to choose between, so
/// they stay newest first.
class ClipsPage extends StatefulWidget {
  const ClipsPage({super.key, this.initialFilters = const []});

  final List<String> initialFilters;

  @override
  State<ClipsPage> createState() => _ClipsPageState();
}

class _ClipsPageState extends State<ClipsPage> {
  late List<String> _filters = List.of(widget.initialFilters);

  List<Clip>? _clips;

  /// Set when the list is one label's playlist.
  Playlist? _playlist;
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
      var clips = await api.clips(labels: _filters);
      Playlist? playlist;
      if (_filters.length == 1) {
        // The playlist is its own route, addressed by label id, and the
        // filter is a name: the clips just fetched carry the id.
        final filter = _filters.single.toLowerCase();
        final label = clips
            .expand((c) => c.labels)
            .where((l) => l.name.toLowerCase() == filter)
            .firstOrNull;
        if (label != null) {
          try {
            playlist = await api.playlist(label.id);
            clips = playlist.clips;
          } on ApiException catch (e) {
            // A server from before playlists: the plain list it is.
            if (e.statusCode != 404) rethrow;
          }
        }
      }
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
      if (playlist != null) {
        // A queue playing from this label is this list, so a reload is
        // how it learns of clips that gained or lost the label elsewhere.
        unawaited(AppScope.of(context)
            .player
            .setQueueTracks(playlist.labelName, clips));
      }
      setState(() {
        _clips = clips;
        _playlist = playlist;
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

  /// Move a row, then tell the server. The list moves first and the
  /// server's answer is what it settles on; a refusal springs it back.
  Future<void> _reorder(int oldIndex, int newIndex) async {
    final playlist = _playlist;
    final from = _clips;
    if (playlist == null || from == null || newIndex == oldIndex) return;

    final to = List.of(from);
    final clip = to.removeAt(oldIndex);
    to.insert(newIndex, clip);
    final afterClipId = newIndex == 0 ? null : to[newIndex - 1].id;

    final session = sessionOf(context);
    final player = AppScope.of(context).player;
    final messenger = ScaffoldMessenger.of(context);
    setState(() => _clips = to);
    unawaited(player.setQueueTracks(playlist.labelName, to));

    try {
      final order =
          await session.api.reorderPlaylist(playlist.labelId, clip.id, afterClipId);
      if (!mounted) return;
      final byId = {for (final c in to) c.id: c};
      final settled = [
        for (final id in order)
          if (byId[id] != null) byId[id]!,
      ];
      setState(() => _clips = settled);
      unawaited(player.setQueueTracks(playlist.labelName, settled));
    } on ApiException catch (e) {
      if (e.isUnauthorized) {
        await session.handleUnauthorized();
        return;
      }
      if (!mounted) return;
      setState(() => _clips = from);
      unawaited(player.setQueueTracks(playlist.labelName, from));
      messenger.showSnackBar(SnackBar(content: Text(e.message)));
      // The server answered, so the list this move was made against is
      // out of date. Show the one that exists.
      if (e.statusCode != null) await _load();
    }
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
        else if (_playlist != null) ...[
          SliverToBoxAdapter(
            child: _PlaylistHeading(playlist: _playlist!, clips: clips),
          ),
          SliverReorderableList(
            itemCount: clips.length,
            onReorderItem: _reorder,
            proxyDecorator: (child, _, _) =>
                Material(elevation: 4, child: child),
            itemBuilder: (context, i) => Column(
              key: ValueKey(clips[i].id),
              mainAxisSize: MainAxisSize.min,
              children: [
                _ClipRow(
                  clip: clips[i],
                  player: player,
                  activeFilters: _filters,
                  onOpen: () => _openClip(clips[i]),
                  onLabelTapped: _addFilter,
                  onPlay: () {
                    final api = sessionOf(context).api;
                    player.playQueue(
                      clips,
                      clips[i].id,
                      source: _playlist!.labelName,
                      urlFor: (c) => api.resolve(c.audioPath),
                      headers: api.authHeaders,
                    );
                  },
                  handle: ReorderableDelayedDragStartListener(
                    index: i,
                    child: const Padding(
                      padding: EdgeInsets.symmetric(horizontal: 8, vertical: 12),
                      child: Icon(Icons.drag_handle, semanticLabel: 'Reorder'),
                    ),
                  ),
                ),
                const Divider(height: 1),
              ],
            ),
          ),
        ] else
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
    this.onPlay,
    this.handle,
  });

  final Clip clip;

  /// Instead of playing the clip on its own.
  final VoidCallback? onPlay;

  /// The drag handle, in a playlist.
  final Widget? handle;
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
                  onPressed: onPlay ??
                      () => player.play(
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
                ?handle,
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

/// The playlist's name, its length in clips, and its length in time.
class _PlaylistHeading extends StatelessWidget {
  const _PlaylistHeading({required this.playlist, required this.clips});

  final Playlist playlist;
  final List<Clip> clips;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final unknown = clips.where((c) => c.duration == null).length;
    final parts = <String>[
      clips.length == 1 ? '1 clip' : '${clips.length} clips',
      if (unknown < clips.length)
        unknown == 0
            ? formatDuration(playlist.total)
            : '${formatDuration(playlist.total)} ($unknown unknown)',
    ];
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
      child: Row(
        children: [
          Icon(Icons.queue_music, size: 18, color: theme.colorScheme.primary),
          const SizedBox(width: 6),
          Text(playlist.labelName, style: theme.textTheme.titleSmall),
          Expanded(
            child: Text(
              '  ·  ${parts.join(' · ')}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.outline),
            ),
          ),
        ],
      ),
    );
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
