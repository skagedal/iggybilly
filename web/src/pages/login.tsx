import { useState } from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";

export default function LoginPage() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const signIn = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.login(username, password);
      // A full load, not a client navigation: crossing the auth
      // boundary should start from a clean document.
      window.location.assign("/");
    } catch (e) {
      setError(
        e instanceof ApiError ? e.message : "Could not reach the server.",
      );
      setBusy(false);
    }
  };

  return (
    <Layout>
      <section className="centered">
        <h1>Sign in</h1>
        {error !== null && <p className="error">{error}</p>}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void signIn();
          }}
        >
          <label>
            Username
            <input
              name="username"
              autoComplete="username"
              required
              autoFocus
              value={username}
              disabled={busy}
              onChange={(e) => setUsername(e.target.value)}
            />
          </label>
          <label>
            Password
            <input
              type="password"
              name="password"
              autoComplete="current-password"
              required
              value={password}
              disabled={busy}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy}>
            Sign in
          </button>
        </form>
      </section>
    </Layout>
  );
}
