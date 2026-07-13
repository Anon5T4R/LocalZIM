import { invoke } from "@tauri-apps/api/core";

export interface ZimInfo {
  id: string;
  path: string;
  fileName: string;
  name: string;
  description: string;
  language: string;
  creator: string;
  date: string;
  entryCount: number;
  articleCount: number | null;
  size: number;
  mainPath: string | null;
  favicon: string | null;
}

export interface Suggestion {
  title: string;
  path: string;
}

export interface FtStatus {
  state: "none" | "building" | "ready" | "error";
  progress: number;
  docs: number | null;
}

export interface FtHit {
  title: string;
  path: string;
  /** HTML escapado pelo backend, com os termos da busca em <b>. */
  snippet: string;
  score: number;
}

export interface FtEvent {
  id: string;
  state: FtStatus["state"];
  progress: number;
  docs?: number;
  error?: string;
}

export const openZim = (path: string) => invoke<ZimInfo>("open_zim", { path });
export const closeZim = (id: string) => invoke<void>("close_zim", { id });
export const zimSuggest = (id: string, query: string, limit = 12) =>
  invoke<Suggestion[]>("zim_suggest", { id, query, limit });
export const zimRandom = (id: string) => invoke<string | null>("zim_random", { id });
export const startupFile = () => invoke<string | null>("startup_file");

export interface CreateZimSpec {
  source: string;
  output: string;
  title: string;
  description?: string;
  language?: string;
  creator?: string;
  mainPage?: string | null;
}

export interface ZimCreateEvent {
  state: "building" | "done" | "error";
  progress: number;
  /** Presente na criação a partir de site: "crawl" (baixando) ou "pack". */
  phase?: "crawl" | "pack";
  pages?: number;
  files?: number;
  queued?: number;
  result?: { entries: number; articles: number; size: number; output: string };
  error?: string;
}

export interface CrawlZimSpec {
  url: string;
  output: string;
  title: string;
  description?: string;
  language?: string;
  creator?: string;
  maxDepth?: number;
  maxPages?: number;
  delayMs?: number;
}

export const createZim = (spec: CreateZimSpec) => invoke<void>("create_zim", { spec });
export const createZimFromSite = (spec: CrawlZimSpec) =>
  invoke<void>("create_zim_from_site", { spec });
export const cancelCreateZim = () => invoke<void>("cancel_create_zim");

// ---------- tradução offline ----------

export type Lang = "pt" | "es" | "en";

export interface LegStatus {
  leg: string;
  installed: boolean;
  downloading: boolean;
  bytes: number;
}

export interface DirectionStatus {
  direction: string;
  /** Modelos necessários; 2 quando pivota pelo inglês (pt↔es). */
  legs: LegStatus[];
}

export interface TranslateModelEvent {
  leg: string;
  state: "downloading" | "ready" | "cancelled" | "error";
  received?: number;
  total?: number;
  error?: string;
}

export const translateStatus = (direction: string) =>
  invoke<DirectionStatus>("translate_status", { direction });
export const translateDownload = (leg: string) => invoke<void>("translate_download", { leg });
export const translateCancelDownload = (leg: string) =>
  invoke<void>("translate_cancel_download", { leg });
export const translateRemove = (leg: string) => invoke<void>("translate_remove", { leg });
export const translatePrepare = (direction: string) =>
  invoke<void>("translate_prepare", { direction });
export const translateTexts = (
  id: string,
  article: string,
  direction: string,
  texts: string[]
) => invoke<string[]>("translate_texts", { id, article, direction, texts });

export const fulltextStatus = (id: string) => invoke<FtStatus>("fulltext_status", { id });
export const fulltextBuild = (id: string) => invoke<void>("fulltext_build", { id });
export const fulltextCancel = (id: string) => invoke<void>("fulltext_cancel", { id });
export const fulltextSearch = (id: string, query: string, limit = 30) =>
  invoke<FtHit[]>("fulltext_search", { id, query, limit });
