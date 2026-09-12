import 'package:flutter/material.dart';

import '../auth/session.dart';
import '../cache/track_cache.dart';
import '../player/audio_engine.dart';
import '../player/player_controller.dart';
import '../player/recovering_engine.dart';
import '../settings.dart';
import 'app_scope.dart';
import 'clip_page.dart';
import 'clips_page.dart';
import 'common.dart';
import 'player_bar.dart';
import 'sign_in_page.dart';
import 'theme.dart';

/// The app.
///
/// Owns the long-lived objects — the session, the player and the clips on
/// disk — and decides which of the two worlds is on screen: signed in, or
/// not. Nothing below has to think about that.
class IggybillyApp extends StatefulWidget {
  const IggybillyApp({
    super.key,
    required this.session,
    this.engine,
    this.cache,
    this.settings,
  });

  final Session session;

  /// Injectable so a test can drive the app without a platform player.
  final AudioEngine? engine;

  /// Injectable for the same reason, and null in tests that have no
  /// business touching the filesystem.
  final TrackCache? cache;
  final Settings? settings;

  @override
  State<IggybillyApp> createState() => _IggybillyAppState();
}

class _IggybillyAppState extends State<IggybillyApp> {
  late final PlayerController _player = PlayerController(
    // A player that has failed once must not stay failed: see
    // [RecoveringAudioEngine], which is the whole of that fix.
    engine: widget.engine ?? RecoveringAudioEngine(JustAudioEngine.new),
    cache: widget.cache,
    settings: widget.settings,
  );

  /// The key for the signed-in navigator, so the player bar — which
  /// lives outside it — can still push a route onto it.
  final _navigatorKey = GlobalKey<NavigatorState>();

  @override
  void initState() {
    super.initState();
    // None awaited: the first frame is the splash below, and each of
    // these flips part of the tree over when it knows something.
    widget.session.restore();
    widget.cache?.open();
    _player.restore();
  }

  @override
  void dispose() {
    _player.dispose();
    widget.session.dispose();
    widget.cache?.dispose();
    super.dispose();
  }

  /// Drop whatever is playing when the user signs out. Audio that
  /// carried on after sign-out would be the app still using a token it
  /// has just thrown away.
  void _onSessionChanged() {
    if (widget.session.status != SessionStatus.signedIn &&
        _player.clip != null) {
      _player.stop();
    }
  }

  @override
  Widget build(BuildContext context) {
    return AppScope(
      session: widget.session,
      player: _player,
      cache: widget.cache,
      child: MaterialApp(
        title: 'iggybilly',
        debugShowCheckedModeBanner: false,
        theme: iggybillyTheme(Brightness.light),
        darkTheme: iggybillyTheme(Brightness.dark),
        home: ListenableBuilder(
          listenable: widget.session,
          builder: (context, _) {
            _onSessionChanged();
            switch (widget.session.status) {
              case SessionStatus.restoring:
                return const Scaffold(body: LoadingBody());
              case SessionStatus.signedOut:
                return const SignInPage();
              case SessionStatus.signedIn:
                return _PlayerErrors(
                  player: _player,
                  child: _SignedIn(
                    navigatorKey: _navigatorKey,
                    player: _player,
                  ),
                );
            }
          },
        ),
      ),
    );
  }
}

/// Says so out loud when a clip would not play.
///
/// The controller has recorded these failures for a long time and nothing
/// ever read them, so a clip that could not load made the bar appear and
/// vanish with no explanation — which is a confusing thing to watch,
/// especially since it is usually worth trying again.
class _PlayerErrors extends StatefulWidget {
  const _PlayerErrors({required this.player, required this.child});

  final PlayerController player;
  final Widget child;

  @override
  State<_PlayerErrors> createState() => _PlayerErrorsState();
}

class _PlayerErrorsState extends State<_PlayerErrors> {
  @override
  void initState() {
    super.initState();
    widget.player.addListener(_onPlayerChanged);
  }

  @override
  void dispose() {
    widget.player.removeListener(_onPlayerChanged);
    super.dispose();
  }

  void _onPlayerChanged() {
    final message = widget.player.error;
    if (message == null) return;
    // After the frame, not during it: a notification can arrive while the
    // tree is being built, and showing a snack bar then is an error.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || widget.player.error == null) return;
      showMessage(context, message, isError: true);
      widget.player.clearError();
    });
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// The signed-in world: a navigator with the player bar pinned beneath
/// it.
///
/// The bar sits outside the navigator on purpose. That is what lets a
/// clip keep playing while you walk from the list to a clip to a wiki
/// page — the same reason the web version moved its player out of the
/// router's outlet.
class _SignedIn extends StatelessWidget {
  const _SignedIn({required this.navigatorKey, required this.player});

  final GlobalKey<NavigatorState> navigatorKey;
  final PlayerController player;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Expanded(
          child: Navigator(
            key: navigatorKey,
            onGenerateRoute: (settings) => MaterialPageRoute(
              settings: settings,
              builder: (_) => const ClipsPage(),
            ),
          ),
        ),
        PlayerBar(
          player: player,
          onOpenClip: (id) => navigatorKey.currentState?.push(
            MaterialPageRoute(builder: (_) => ClipPage(clipId: id)),
          ),
        ),
      ],
    );
  }
}
