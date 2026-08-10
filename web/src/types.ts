// Mirrors the `Serialize` structs on the Rust side (all `rename_all =
// "camelCase"`). Keep the two in sync by hand — the surface is small
// enough that a codegen step would cost more than it saves.

/** A label as shown on a clip in the list: name plus the filter link. */
export interface LabelLink {
  name: string;
  href: string;
}

/** A label on the clip detail page, where it can also be removed. */
export interface ClipLabel {
  id: number;
  name: string;
  filterHref: string;
}

export interface FilterChip {
  name: string;
  /** Link to the same list with just this one filter dropped. */
  removeHref: string;
}

export interface ClipSummary {
  id: number;
  name: string;
  originalFilename: string;
  uploadedAt: string;
  recordingDate: string | null;
  uploader: string;
  labels: LabelLink[];
  /** Precomputed waveform samples, or null when we couldn't decode. */
  peaks: number[] | null;
  durationSeconds: number | null;
}

export interface ClipDetail {
  id: number;
  name: string;
  originalFilename: string;
  contentType: string;
  uploader: string;
  uploadedAt: string;
  recordingDate: string | null;
  labels: ClipLabel[];
  peaks: number[] | null;
  durationSeconds: number | null;
  /** True only for the uploader — you can delete your own clips. */
  canDelete: boolean;
}

export interface WikiPage {
  labelId: number;
  labelName: string;
  /** Raw Markdown source, for the editor. */
  content: string;
  /** Rendered, sanitised HTML. Safe to inject. */
  contentHtml: string;
  hasContent: boolean;
  /** "edited by <user> on <date time>", or null when there's no page. */
  lastEdited: string | null;
}

export interface WikiRevision {
  id: number;
  author: string;
  editedAt: string;
  contentHtml: string;
  isCurrent: boolean;
}

export interface LabelSearchResult {
  query: string;
  matches: string[];
  /** Whether to offer "create new label" for the current query. */
  canCreate: boolean;
}

// --- Per-page props, embedded in the HTML by the server ---------------

export interface IndexProps {
  username: string;
  clips: ClipSummary[];
  activeFilters: FilterChip[];
  activeWikis: WikiPage[];
}

export interface ClipProps {
  username: string;
  clip: ClipDetail;
}

export interface WikiHistoryProps {
  username: string;
  labelId: number;
  labelName: string;
  revisions: WikiRevision[];
}

export interface AccountProps {
  username: string;
}

export type LoginProps = Record<string, never>;
