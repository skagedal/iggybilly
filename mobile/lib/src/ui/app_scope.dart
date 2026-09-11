import 'package:flutter/widgets.dart';

import '../auth/session.dart';
import '../player/player_controller.dart';

/// The two long-lived objects every screen needs, handed down the tree.
///
/// An InheritedWidget rather than a state-management package: there are
/// exactly two of them, they live for the life of the app, and neither
/// is rebuilt — what changes is what they notify, which the screens
/// listen to themselves.
class AppScope extends InheritedWidget {
  const AppScope({
    super.key,
    required this.session,
    required this.player,
    required super.child,
  });

  final Session session;
  final PlayerController player;

  /// Look the scope up without depending on it.
  ///
  /// `getInheritedWidgetOfExactType` rather than `dependOn…`: these two
  /// objects are created once and never replaced, so there is nothing to
  /// be rebuilt for. Not registering a dependency is also what makes
  /// this legal from `initState`, which is where a screen kicks off its
  /// first load. Screens that need to rebuild when something changes
  /// listen to the object itself, which is a ChangeNotifier.
  static AppScope of(BuildContext context) {
    final scope = context.getInheritedWidgetOfExactType<AppScope>();
    assert(scope != null, 'No AppScope above this widget');
    return scope!;
  }

  @override
  bool updateShouldNotify(AppScope old) => false;
}
