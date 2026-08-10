import { useState } from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";
import type { AccountProps } from "../types";

export default function AccountPage({ username }: AccountProps) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setError(null);
    setSuccess(null);
    // Checked here for an instant answer; the server checks again.
    if (next !== confirm) {
      setError("New passwords don't match.");
      return;
    }
    setBusy(true);
    try {
      await api.changePassword(current, next);
      setSuccess("Password updated.");
      setCurrent("");
      setNext("");
      setConfirm("");
    } catch (e) {
      setError(
        e instanceof ApiError ? e.message : "Could not update the password.",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Layout username={username} nav={<a href="/">Clips</a>}>
      <section className="centered">
        <h1>Change password</h1>
        {error !== null && <p className="error">{error}</p>}
        {success !== null && <p className="success">{success}</p>}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <label>
            Current password
            <input
              type="password"
              autoComplete="current-password"
              required
              value={current}
              disabled={busy}
              onChange={(e) => setCurrent(e.target.value)}
            />
          </label>
          <label>
            New password
            <input
              type="password"
              autoComplete="new-password"
              required
              minLength={10}
              value={next}
              disabled={busy}
              onChange={(e) => setNext(e.target.value)}
            />
          </label>
          <label>
            Confirm new password
            <input
              type="password"
              autoComplete="new-password"
              required
              minLength={10}
              value={confirm}
              disabled={busy}
              onChange={(e) => setConfirm(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy}>
            Update password
          </button>
        </form>
      </section>
    </Layout>
  );
}
