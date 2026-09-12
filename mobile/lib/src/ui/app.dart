import 'package:flutter/material.dart';

import '../auth/session.dart';
import '../player/audio_engine.dart';
import '../player/player_controller.dart';
import 'app_scope.dart';
import 'clip_page.dart';
import 'clips_page.dart';
import 'common.dart';
import 'player_bar.dart';
import 'sign_in_page.dart';
import 'theme.dart';

/// The app.
///
/// Owns the two long-lived objects — the session and the player — and
/// decides which of the two worlds is on screen: signed in, or not.
/// Nothing below has to think about that.
class IggybillyApp extends StatefulWidget {
  const IggybillyApp({super.key, required this.session, this.engine});

  final Session session;

  /// Injectable so a test can drive the app without a platform player.
  final AudioEngine? engine;

  @override
  State<IggybillyApp> createState() => _IggybillyAppState();
}

class _IggybillyAppState extends State<IggybillyApp> {
  late final PlayerController _player = PlayerController(
    engine: widget.engine ?? JustAudioEngine(),
  );

  /// The key for the signed-in navigator, so the player bar — which
  /// lives outside it — can still push a route onto it.
  final _navigatorKey = GlobalKey<NavigatorState>();

  @override
  void initState() {
    super.initState();
    // Not awaited: the first frame is the splash below, and the session
    // flips the tree over when it knows.
    widget.session.restore();
  }

  @override
  void dispose() {
    _player.dispose();
    widget.session.dispose();
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
                return _SignedIn(
                  navigatorKey: _navigatorKey,
                  player: _player,
                );
            }
          },
        ),
      ),
    );
  }
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
