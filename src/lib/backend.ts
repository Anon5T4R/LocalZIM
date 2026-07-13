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
  result?: { entries: number; articles: number; size: number; output: string };
  error?: string;
}

export const createZim = (spec: CreateZimSpec) => invoke<void>("create_zim", { spec });
export const cancelCreateZim = () => invoke<void>("cancel_create_zim");

export const fulltextStatus = (id: string) => invoke<FtStatus>("fulltext_status", { id });
export const fulltextBuild = (id: string) => invoke<void>("fulltext_build", { id });
export const fulltextCancel = (id: string) => invoke<void>("fulltext_cancel", { id });
export const fulltextSearch = (id: string, query: string, limit = 30) =>
  invoke<FtHit[]>("fulltext_search", { id, query, limit });
