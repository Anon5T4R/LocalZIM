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

export const openZim = (path: string) => invoke<ZimInfo>("open_zim", { path });
export const closeZim = (id: string) => invoke<void>("close_zim", { id });
export const zimSuggest = (id: string, query: string, limit = 12) =>
  invoke<Suggestion[]>("zim_suggest", { id, query, limit });
export const zimRandom = (id: string) => invoke<string | null>("zim_random", { id });
export const startupFile = () => invoke<string | null>("startup_file");
