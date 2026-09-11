import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

import { loadPage } from "./pageData";
import type { PageComponent, PageEnvelope } from "./pageData";

/**
 * Client-side navigation over server-defined routes.
 *
 * The client holds no route table. To go somewhere it fetches that exact
 * URL with `Accept: application/json`; the server answers with the same
 * `{entry, title, props}` envelope it would have embedded in a full page
 * load, and we render it. So URLs stay the server's business, every one
 * of them is directly loadable, and adding a route needs no change here.
 *
 * The point of doing this at all is that the document survives: the
 * global player sits above the outlet and keeps playing across
 * navigation, which a real page load could never allow.
 *
 * Anything we can't handle — a redirect to /login, a non-JSON answer, a
 * network failure — falls back to a hard navigation, so a bug in here
 * degrades to the plain multi-page behaviour rather than a dead link.
 */

interface RouterContextValue {
  /**
   * Go to `href`. Pass `replace` when the destination supersedes the
   * current entry rather than following it — re-fetching the page you're
   * already on, say — so Back doesn't have to step through duplicates.
   */
  navigate: (href: string, options?: { replace?: boolean }) => void;
  /** True while a navigation is in flight, for the progress bar. */
  pending: boolean;
}

const RouterContext = createContext<RouterContextValue | null>(null);

export function useRouter(): RouterContextValue {
  const ctx = useContext(RouterContext);
  if (!ctx) throw new Error("useRouter must be used inside <Router>");
  return ctx;
}

interface RouterProps {
  initialEnvelope: PageEnvelope;
  initialComponent: PageComponent;
}

interface RouteState {
  envelope: PageEnvelope;
  Component: PageComponent;
}

export function Router({ initialEnvelope, initialComponent }: RouterProps) {
  const [route, setRoute] = useState<RouteState>({
    envelope: initialEnvelope,
    Component: initialComponent,
  });
  const [pending, setPending] = useState(false);
  // Bumped on every navigation so a slow fetch that lost the race can
  // tell it's been superseded and drop its result.
  const generation = useRef(0);

  const show = useCallback((next: RouteState) => {
    setRoute(next);
    document.title = next.envelope.title;
  }, []);

  const go = useCallback(
    async (url: string, mode: "push" | "replace" | "pop") => {
      const attempt = ++generation.current;
      setPending(true);
      try {
        const res = await fetch(url, {
          headers: { Accept: "application/json" },
          credentials: "same-origin",
        });

        // A redirect means the server sent us somewhere else — an
        // expired session bouncing to /login, most likely. fetch follows
        // it transparently, so honour it as a real navigation instead of
        // rendering the destination under the wrong URL.
        if (res.redirected || !res.ok) {
          window.location.assign(res.redirected ? res.url : url);
          return;
        }

        const envelope = (await res.json()) as PageEnvelope;
        const Component = await loadPage(envelope.entry);
        if (attempt !== generation.current) return;

        // Save where we were before leaving, so going back restores it.
        if (mode === "push") {
          window.history.replaceState(
            { scrollY: window.scrollY },
            "",
            window.location.href,
          );
          window.history.pushState({ scrollY: 0 }, "", url);
        } else if (mode === "replace") {
          window.history.replaceState({ scrollY: 0 }, "", url);
        }

        show({ envelope, Component });

        if (mode === "pop") {
          const saved = (window.history.state as { scrollY?: number } | null)
            ?.scrollY;
          window.scrollTo(0, saved ?? 0);
        } else {
          window.scrollTo(0, 0);
        }
      } catch {
        // Network error, non-JSON body, unknown entry — let the browser
        // do it the old-fashioned way.
        window.location.assign(url);
      } finally {
        if (attempt === generation.current) setPending(false);
      }
    },
    [show],
  );

  const navigate = useCallback(
    (href: string, options?: { replace?: boolean }) =>
      void go(href, options?.replace ? "replace" : "push"),
    [go],
  );

  // Browsers restore scroll themselves on back/forward, which fights
  // with rendering the page asynchronously — we do it in `go` instead.
  useEffect(() => {
    const previous = window.history.scrollRestoration;
    window.history.scrollRestoration = "manual";
    return () => {
      window.history.scrollRestoration = previous;
    };
  }, []);

  useEffect(() => {
    const onPopState = () => void go(window.location.href, "pop");
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, [go]);

  // One delegated listener beats threading an onClick through every
  // link — including links inside rendered wiki Markdown, which we
  // don't control.
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      const href = interceptableHref(e);
      if (href === null) return;
      e.preventDefault();
      void go(href, "push");
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, [go]);

  const { Component, envelope } = route;
  return (
    <RouterContext.Provider value={{ navigate, pending }}>
      {pending && <div className="nav-progress" />}
      {/* Keyed by entry so switching page kinds remounts rather than
          trying to reconcile two unrelated trees. */}
      <Component
        key={envelope.entry}
        {...(envelope.props as Record<string, unknown>)}
      />
    </RouterContext.Provider>
  );
}

/**
 * The URL a click should be turned into in-app navigation for, or null
 * to leave it to the browser.
 *
 * Deliberately permissive about *which* paths qualify: we don't know
 * which URLs are pages, and we don't want to duplicate the server's
 * routing to find out. Anything that isn't a page comes back as
 * non-JSON, and `go` falls back to a hard navigation.
 */
function interceptableHref(e: MouseEvent): string | null {
  // Modified clicks are the user asking for a new tab/window/download.
  if (e.defaultPrevented || e.button !== 0) return null;
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return null;

  const target = e.target;
  if (!(target instanceof Element)) return null;
  const anchor = target.closest("a");
  if (!anchor || !anchor.href) return null;

  // `download` covers the clip download links, which must stay real
  // requests; an explicit target wants a different browsing context.
  if (anchor.hasAttribute("download")) return null;
  if (anchor.target && anchor.target !== "_self") return null;
  if (anchor.getAttribute("rel")?.split(/\s+/).includes("external")) {
    return null;
  }

  const url = new URL(anchor.href, window.location.href);
  if (url.origin !== window.location.origin) return null;
  // A bare #fragment is the browser's job, not ours.
  if (
    url.pathname === window.location.pathname &&
    url.search === window.location.search &&
    url.hash
  ) {
    return null;
  }
  return url.pathname + url.search;
}
