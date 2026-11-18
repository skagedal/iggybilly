import { useState } from "react";

import { ApiError, api } from "../api";
import { Layout } from "../components/Layout";
import { useRouter } from "../router";
import type { AccountProps } from "../types";

/** "Clips" links back into the band you are in, when you are in one. */
function bandNav(band: string | null) {
  if (band === null) return undefined;
  return <a href={`/${band}/`}>Clips</a>;
}

export default function AccountPage({ username, bands, band }: AccountProps) {
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
    <Layout username={username} band={band ?? undefined} nav={bandNav(band)}>
      <section className="centered">
        <Bands bands={bands} current={band} />
      </section>
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

/**
 * The bands you are in, and which one you are looking at.
 *
 * One band names it and offers nothing to pick, because there would be
 * nothing in the picker. None says so. Several give you the switch, and
 * this is the only place it lives — there is no band menu in the header,
 * since for all but one or two people there would never be anything in
 * that either.
 */
function Bands({
  bands,
  current,
}: {
  bands: string[];
  current: string | null;
}) {
  const { navigate } = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (bands.length === 0) {
    return (
      <>
        <h1>Band</h1>
        <p>
          You are not in a band. Ask whoever runs this iggybilly to add you
          to one.
        </p>
      </>
    );
  }

  if (bands.length === 1) {
    return (
      <>
        <h1>Band</h1>
        <p>
          You are in <strong>{bands[0]}</strong>.
        </p>
      </>
    );
  }

  const switchTo = async (band: string) => {
    if (band === current || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.switchBand(band);
      navigate(`/${band}/`);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not switch band.");
      setBusy(false);
    }
  };

  return (
    <>
      <h1>Band</h1>
      {error !== null && <p className="error">{error}</p>}
      <ul className="bands">
        {bands.map((band) => (
          <li key={band}>
            {band === current ? (
              <strong>{band}</strong>
            ) : (
              <button
                type="button"
                className="link"
                disabled={busy}
                onClick={() => void switchTo(band)}
              >
                {band}
              </button>
            )}
          </li>
        ))}
      </ul>
    </>
  );
}
