// Utilitários puros de URL do protocolo zim:// (testáveis fora do Tauri).
// No Windows o WebView2 expõe protocolos customizados como http://<scheme>.localhost/.

export function isWindowsUA(): boolean {
  return typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
}

export function zimBase(): string {
  return isWindowsUA() ? "http://zim.localhost/" : "zim://localhost/";
}

/** Escapa cada segmento do caminho da entrada, preservando as barras. */
export function encodeEntryPath(p: string): string {
  return p.split("/").map(encodeURIComponent).join("/");
}

/** URL servível de uma entrada ("N/url") de um arquivo ZIM aberto. */
export function zimUrl(id: string, entryPath: string): string {
  return zimBase() + id + "/" + encodeEntryPath(entryPath);
}

/** O inverso: extrai (id, caminho) de um href do protocolo zim. */
export function pathFromHref(href: string): { id: string; path: string } | null {
  try {
    const u = new URL(href);
    const p = u.pathname.replace(/^\//, "");
    const i = p.indexOf("/");
    if (i <= 0) return null;
    return { id: p.slice(0, i), path: decodeURIComponent(p.slice(i + 1)) };
  } catch {
    return null;
  }
}

export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 10 || u === 0 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}
