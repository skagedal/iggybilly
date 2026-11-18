// Typed wrappers around the JSON API under /api.
//
// Session auth rides on the cookie, which is SameSite=Strict — so a
// cross-site page can't make these calls carry credentials, and we need
// no separate CSRF token. `credentials: "same-origin"` is fetch's
// default but is spelled out here because it's load-bearing.

import type {
  ClipLabel,
  LabelSearchResult,
  WikiPage,
} from "./types";

/** An API call that came back with a non-2xx status. */
export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const init: RequestInit = {
    method,
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  };
  if (body instanceof FormData) {
    init.body = body;
  } else if (body !== undefined) {
    init.headers = { ...init.headers, "Content-Type": "application/json" };
    init.body = JSON.stringify(body);
  }

  const res = await fetch(path, init);

  if (!res.ok) {
    // Errors are `{"error": "..."}`, but a proxy or a panic could return
    // something else — fall back to the status text rather than throwing
    // a parse error that hides the real failure.
    let message = res.statusText || `request failed (${res.status})`;
    try {
      const parsed = (await res.json()) as { error?: unknown };
      if (typeof parsed.error === "string") message = parsed.error;
    } catch {
      /* not JSON — keep the status text */
    }
    throw new ApiError(res.status, message);
  }

  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

const get = <T>(path: string) => request<T>("GET", path);
const post = <T>(path: string, body?: unknown) =>
  request<T>("POST", path, body);
const del = <T>(path: string) => request<T>("DELETE", path);

/**
 * The first path segments that are not a band: the screens about the
 * person or the server rather than about one band's content.
 */
const NOT_A_BAND = new Set(["login", "account", "api", "static", "healthz"]);

/**
 * The band the current page belongs to, read from the URL.
 *
 * Every band page's URL begins with the slug, which makes the URL the
 * one place this is always right — including immediately after a
 * client-side navigation, since the router pushes the new URL before
 * rendering the page under it. The alternative, threading a band down
 * from page props, would mean passing one through every component that
 * happens to make a call: the label input, the wiki panel, the player.
 *
 * Throwing rather than guessing matters. A wrong slug would build a URL
 * that looks plausible and quietly asks about the wrong band.
 */
function currentBand(): string {
  const slug = window.location.pathname.split("/")[1] ?? "";
  if (!slug || NOT_A_BAND.has(slug)) {
    throw new Error(
      `no band in the URL (${window.location.pathname}): this call is about a band's content and can only be made from one of its pages`,
    );
  }
  return slug;
}

/** `/api/<band>/…`, for the calls that are about a band's content. */
const bandPath = (path: string) =>
  `/api/${encodeURIComponent(currentBand())}${path}`;

export const api = {
  login: (username: string, password: string) =>
    post<void>("/api/login", { username, password }),

  logout: () => post<void>("/api/logout"),

  changePassword: (currentPassword: string, newPassword: string) =>
    post<void>("/api/account/password", { currentPassword, newPassword }),

  /**
   * Remember which band the user is looking at. The account screen posts
   * this and then navigates to the band, which would set it anyway —
   * posting first means the preference is right even if the navigation is
   * the last thing this tab does.
   */
  switchBand: (band: string) => post<void>("/api/account/band", { band }),

  /** Uploads one or more files in a single multipart request. */
  uploadClips: (files: File[]) => {
    const form = new FormData();
    for (const file of files) form.append("audio", file, file.name);
    return post<{ clips: { id: number; name: string }[] }>(bandPath("/clips"), form);
  },

  deleteClip: (clipId: number) => del<void>(bandPath(`/clips/${clipId}`)),

  renameClip: (clipId: number, name: string) =>
    post<{ name: string }>(bandPath(`/clips/${clipId}/name`), { name }),

  addLabel: (clipId: number, name: string) =>
    post<ClipLabel[]>(bandPath(`/clips/${clipId}/labels`), { name }),

  removeLabel: (clipId: number, labelId: number) =>
    del<ClipLabel[]>(bandPath(`/clips/${clipId}/labels/${labelId}`)),

  searchLabels: (query: string, clipId?: number) => {
    const params = new URLSearchParams({ q: query });
    if (clipId !== undefined) params.set("clip_id", String(clipId));
    return get<LabelSearchResult>(bandPath(`/labels/search?${params}`));
  },

  /**
   * Move a clip to just after `afterClipId` in a label's playlist, or to
   * the front when that is null. Answers the label's whole order as it
   * now stands.
   */
  reorderPlaylist: (labelId: number, clipId: number, afterClipId: number | null) =>
    post<{ order: number[] }>(bandPath(`/labels/${labelId}/order`), {
      clipId,
      afterClipId,
    }),

  getWiki: (labelId: number) => get<WikiPage>(bandPath(`/labels/${labelId}/wiki`)),

  saveWiki: (labelId: number, content: string) =>
    post<WikiPage>(bandPath(`/labels/${labelId}/wiki`), { content }),

  restoreWikiRevision: (labelId: number, revisionId: number) =>
    post<void>(bandPath(`/labels/${labelId}/wiki/restore/${revisionId}`)),
};
