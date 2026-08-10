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

export const api = {
  login: (username: string, password: string) =>
    post<void>("/api/login", { username, password }),

  logout: () => post<void>("/api/logout"),

  changePassword: (currentPassword: string, newPassword: string) =>
    post<void>("/api/account/password", { currentPassword, newPassword }),

  /** Uploads one or more files in a single multipart request. */
  uploadClips: (files: File[]) => {
    const form = new FormData();
    for (const file of files) form.append("audio", file, file.name);
    return post<{ clips: { id: number; name: string }[] }>("/api/clips", form);
  },

  deleteClip: (clipId: number) => del<void>(`/api/clips/${clipId}`),

  renameClip: (clipId: number, name: string) =>
    post<{ name: string }>(`/api/clips/${clipId}/name`, { name }),

  addLabel: (clipId: number, name: string) =>
    post<ClipLabel[]>(`/api/clips/${clipId}/labels`, { name }),

  removeLabel: (clipId: number, labelId: number) =>
    del<ClipLabel[]>(`/api/clips/${clipId}/labels/${labelId}`),

  searchLabels: (query: string, clipId?: number) => {
    const params = new URLSearchParams({ q: query });
    if (clipId !== undefined) params.set("clip_id", String(clipId));
    return get<LabelSearchResult>(`/api/labels/search?${params}`);
  },

  getWiki: (labelId: number) => get<WikiPage>(`/api/labels/${labelId}/wiki`),

  saveWiki: (labelId: number, content: string) =>
    post<WikiPage>(`/api/labels/${labelId}/wiki`, { content }),

  restoreWikiRevision: (labelId: number, revisionId: number) =>
    post<void>(`/api/labels/${labelId}/wiki/restore/${revisionId}`),
};
