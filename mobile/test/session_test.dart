import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:iggybilly/src/api/client.dart';
import 'package:iggybilly/src/auth/session.dart';

import 'fakes.dart';

Session sessionWith(FakeHttpClient client, {InMemoryCredentialStore? store}) =>
    Session(
      store: store ?? InMemoryCredentialStore(),
      httpClient: client,
      deviceName: () async => 'Test phone',
      defaultServer: Uri.parse('https://default.test'),
    );

void main() {
  test('with no stored token, the app starts signed out', () async {
    final session = sessionWith(FakeHttpClient.json(null));
    await session.restore();
    expect(session.status, SessionStatus.signedOut);
    expect(session.server.toString(), 'https://default.test');
  });

  test('a stored token that still works signs the app straight in', () async {
    final session = sessionWith(
      FakeHttpClient.json(userJson(username: 'alice')),
      store: storeWithToken('tok'),
    );
    await session.restore();
    expect(session.status, SessionStatus.signedIn);
    expect(session.user!.username, 'alice');
    expect(session.server.toString(), 'https://example.test');
  });

  test('a revoked token is discarded rather than kept around', () async {
    final store = storeWithToken('revoked');
    final session = sessionWith(
      FakeHttpClient((_) => fails(401, 'invalid or revoked token')),
      store: store,
    );
    await session.restore();
    expect(session.status, SessionStatus.signedOut);
    expect(store.token, isNull, reason: 'a dead token is not worth storing');
  });

  test('an unreachable server does not sign the user out', () async {
    // The difference that matters: a tunnel or a plane must not cost the
    // user a credential they still hold.
    final store = storeWithToken('tok');
    final session = sessionWith(
      FakeHttpClient((_) => throw http.ClientException('no route to host')),
      store: store,
    );
    await session.restore();
    expect(session.status, SessionStatus.signedIn);
    expect(store.token, 'tok');
  });

  test('signing in stores the token, the server and the username', () async {
    final store = InMemoryCredentialStore();
    final session = sessionWith(
      FakeHttpClient.json({'token': 'new', 'tokenId': 1, 'user': userJson()}),
      store: store,
    );
    await session.signIn(
      server: Uri.parse('https://band.test'),
      username: 'alice',
      password: 'pw',
    );

    expect(session.status, SessionStatus.signedIn);
    expect(store.token, 'new');
    expect(store.serverUrl, 'https://band.test');
    expect(store.username, 'alice');
    expect(session.api.token, 'new');
  });

  test(
    'a failed sign-in leaves the app signed out and stores nothing',
    () async {
      final store = InMemoryCredentialStore();
      final session = sessionWith(
        FakeHttpClient((_) => fails(401, 'Invalid username or password.')),
        store: store,
      );

      await expectLater(
        session.signIn(
          server: Uri.parse('https://band.test'),
          username: 'alice',
          password: 'wrong',
        ),
        throwsA(
          isA<ApiException>().having(
            (e) => e.message,
            'message',
            'Invalid username or password.',
          ),
        ),
      );
      expect(session.status, SessionStatus.restoring);
      expect(store.token, isNull);
    },
  );

  test(
    'the device name reaches the server so the token is recognisable',
    () async {
      final fake = FakeHttpClient.json({
        'token': 'new',
        'tokenId': 1,
        'user': userJson(),
      });
      await sessionWith(fake).signIn(
        server: Uri.parse('https://band.test'),
        username: 'alice',
        password: 'pw',
      );
      expect(fake.lastRequest.body, contains('Test phone'));
    },
  );

  test('signing out drops the credential even if the server refuses', () async {
    final store = storeWithToken('tok');
    var calls = 0;
    final session = sessionWith(
      FakeHttpClient((request) {
        calls++;
        if (request.url.path == '/api/v1/me') return ok(userJson());
        throw http.ClientException('offline');
      }),
      store: store,
    );
    await session.restore();
    await session.signOut();

    expect(calls, greaterThan(1), reason: 'it did try to revoke server-side');
    expect(session.status, SessionStatus.signedOut);
    expect(store.token, isNull);
  });

  test('changing the password keeps the replacement token', () async {
    final store = storeWithToken('old');
    final session = sessionWith(
      FakeHttpClient(
        (request) => request.url.path == '/api/v1/me'
            ? ok(userJson())
            : ok({'token': 'replacement', 'tokenId': 9}),
      ),
      store: store,
    );
    await session.restore();
    await session.changePassword(
      currentPassword: 'pw',
      newPassword: 'a-much-longer-one',
    );

    // The server revoked every token including the one that made the
    // call, so keeping the old one would sign the user out a request
    // later, with no way to explain it.
    expect(store.token, 'replacement');
    expect(session.api.token, 'replacement');
    expect(session.status, SessionStatus.signedIn);
  });

  test('a 401 mid-session returns the app to the sign-in screen', () async {
    final store = storeWithToken('tok');
    final session = sessionWith(FakeHttpClient.json(userJson()), store: store);
    await session.restore();
    expect(session.status, SessionStatus.signedIn);

    await session.handleUnauthorized();
    expect(session.status, SessionStatus.signedOut);
    expect(store.token, isNull);
  });

  test('reading the client while signed out is a programming error', () async {
    final session = sessionWith(FakeHttpClient.json(null));
    await session.restore();
    expect(() => session.api, throwsStateError);
  });

  test('listeners hear every status change', () async {
    final session = sessionWith(FakeHttpClient.json(null));
    final seen = <SessionStatus>[];
    session.addListener(() => seen.add(session.status));
    await session.restore();
    expect(seen, [SessionStatus.signedOut]);
  });

  // -- Bands ----------------------------------------------------------

  test('the band on this device survives a restart', () async {
    final store = storeWithToken('tok')..band = 'bonband';
    final session = sessionWith(
      FakeHttpClient.json(userJson(bands: ['bonband', 'default'])),
      store: store,
    );
    await session.restore();
    expect(
      session.band,
      'bonband',
      reason: 'the device remembers, rather than taking the server default',
    );
  });

  test('a stored band the user has left is dropped', () async {
    // Removed from bonband since this phone last looked at it. Following
    // the stored slug would mean every request 404ing.
    final store = storeWithToken('tok')..band = 'bonband';
    final session = sessionWith(
      FakeHttpClient.json(userJson(bands: ['default'], band: 'default')),
      store: store,
    );
    await session.restore();
    expect(session.band, 'default');
    expect(store.band, 'default', reason: 'and the device is corrected');
  });

  test('an account in no band restores with none', () async {
    final store = storeWithToken('tok');
    final session = sessionWith(
      FakeHttpClient.json(userJson(bands: const [], band: null)),
      store: store,
    );
    await session.restore();
    expect(session.status, SessionStatus.signedIn);
    expect(session.band, isNull);
    expect(session.bands, isEmpty);
  });

  test('switching band tells the server before it takes effect', () async {
    final store = storeWithToken('tok')..band = 'default';
    final fake = FakeHttpClient((request) {
      if (request.url.path == '/api/v1/me') {
        return http.Response(
          jsonEncode(userJson(bands: ['bonband', 'default'])),
          200,
          headers: {'content-type': 'application/json'},
        );
      }
      return http.Response('', 204);
    });
    final session = sessionWith(fake, store: store);
    await session.restore();

    await session.switchBand('bonband');
    expect(fake.lastRequest.url.path, '/api/v1/me/band');
    expect(session.band, 'bonband');
    expect(store.band, 'bonband', reason: 'and is remembered on the device');
  });

  test('signing out forgets the band as well as the token', () async {
    final store = storeWithToken('tok')..band = 'default';
    final session = sessionWith(FakeHttpClient.json(userJson()), store: store);
    await session.restore();
    await session.signOut();
    expect(store.token, isNull);
    expect(
      store.band,
      isNull,
      reason: 'the next person to sign in starts in their own band',
    );
  });
}
