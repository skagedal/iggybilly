import type { ComponentType } from "react";

/**
 * What the server says a URL is: which page module renders it, what the
 * document is called, and that module's data.
 *
 * The identical shape arrives two ways — embedded in the HTML shell on a
 * cold load, and as a JSON body when the router fetches a page it's
 * about to swap in. See `page()` in src/handlers/mod.rs.
 */
export interface PageEnvelope {
  entry: string;
  title: string;
  props: unknown;
}

/**
 * A page module's default export.
 *
 * The props are genuinely `any` here: the server and the page agree on
 * their shape, and the router only ferries them across without ever
 * looking inside. Each page declares its own real props type.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type PageComponent = ComponentType<any>;

/**
 * The page modules, keyed by the `entry` the server sends.
 *
 * These are dynamic imports, so esbuild splits each page into its own
 * chunk and the browser only downloads the ones actually visited.
 * Adding a page means adding a file under pages/ and a line here.
 */
const pages: Record<string, () => Promise<{ default: PageComponent }>> = {
  index: () => import("./pages/index"),
  clip: () => import("./pages/clip"),
  "wiki-history": () => import("./pages/wiki-history"),
  login: () => import("./pages/login"),
  account: () => import("./pages/account"),
};

export async function loadPage(entry: string): Promise<PageComponent> {
  const load = pages[entry];
  if (!load) throw new Error(`unknown page entry: ${entry}`);
  return (await load()).default;
}

/** Read the envelope the server embedded in the shell. */
export function readInitialEnvelope(): PageEnvelope {
  const el = document.getElementById("page-data");
  if (!el?.textContent) throw new Error("page shell is missing #page-data");
  return JSON.parse(el.textContent) as PageEnvelope;
}
