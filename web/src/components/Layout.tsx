import { useState } from "react";
import type { ReactNode } from "react";

import { api } from "../api";

interface LayoutProps {
  /** Omitted on the login page, which has no signed-in user. */
  username?: string;
  /**
   * The band this screen belongs to, when it belongs to one. The logo
   * links to its clip list; without a band it links to `/`, which
   * redirects to whichever band you were last in.
   */
  band?: string;
  /** Route-specific nav links, rendered between the name and Sign out. */
  nav?: ReactNode;
  children: ReactNode;
}

export function Layout({ username, band, nav, children }: LayoutProps) {
  return (
    <>
      <header>
        <a className="logo" href={band === undefined ? "/" : `/${band}/`}>
          iggybilly
        </a>
        <nav>
          {username !== undefined && (
            <span className="username">{username}</span>
          )}
          {nav}
          {username !== undefined && <SignOutButton />}
        </nav>
      </header>
      <main>{children}</main>
    </>
  );
}

function SignOutButton() {
  const [busy, setBusy] = useState(false);

  const signOut = async () => {
    setBusy(true);
    try {
      await api.logout();
    } finally {
      // Even if the call failed, send the user to the login page — the
      // session is either gone or unusable, and staying put is worse.
      window.location.assign("/login");
    }
  };

  return (
    <button type="button" className="link" disabled={busy} onClick={signOut}>
      Sign out
    </button>
  );
}
