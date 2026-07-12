import type { ZimInfo } from "./backend";

// Histórico de arquivos abertos, persistido em localStorage.

export interface RecentBook {
  path: string;
  name: string;
  description: string;
  language: string;
  size: number;
  articleCount: number | null;
  favicon: string | null;
  lastOpened: number;
}

const KEY = "localzim.recents";
const MAX = 24;

export function loadRecents(): RecentBook[] {
  try {
    const raw = localStorage.getItem(KEY);
    const list = raw ? (JSON.parse(raw) as RecentBook[]) : [];
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

export function saveRecent(info: ZimInfo): RecentBook[] {
  const entry: RecentBook = {
    path: info.path,
    name: info.name,
    description: info.description,
    language: info.language,
    size: info.size,
    articleCount: info.articleCount,
    favicon: info.favicon,
    lastOpened: Date.now(),
  };
  const list = [entry, ...loadRecents().filter((r) => r.path !== info.path)].slice(0, MAX);
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // favicons grandes podem estourar a cota — tenta de novo sem eles
    try {
      localStorage.setItem(KEY, JSON.stringify(list.map((r) => ({ ...r, favicon: null }))));
    } catch {}
  }
  return list;
}

export function removeRecent(path: string): RecentBook[] {
  const list = loadRecents().filter((r) => r.path !== path);
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {}
  return list;
}
