/**
 * Temas da UI. Além de `light`/`dark` há 5 temas nomeados com paleta fixa
 * (3 claros, 2 escuros) — o valor vai direto pro `data-theme` do `<html>`,
 * que o `styles.css` usa pra redefinir as vars de cor.
 *
 * O conteúdo do .zim renderiza num iframe com CSS próprio: ele só recebe o
 * modo claro/escuro (`isDarkTheme`), nunca a paleta do chrome.
 */

export type Theme =
  | "light"
  | "dark"
  | "nature"
  | "darkblue"
  | "calmgreen"
  | "pastelpink"
  | "punkprincess";

export const THEMES: readonly Theme[] = [
  "light",
  "dark",
  "nature",
  "darkblue",
  "calmgreen",
  "pastelpink",
  "punkprincess",
] as const;

const THEME_KEY = "localzim.theme";

/** Temas cujo artigo deve ser renderizado em modo escuro. */
const DARK_THEMES: readonly Theme[] = ["dark", "darkblue", "punkprincess"];

/** Modo escuro derivado do tema (o iframe do artigo só conhece claro/escuro). */
export function isDarkTheme(theme: Theme): boolean {
  return DARK_THEMES.includes(theme);
}

function isTheme(v: unknown): v is Theme {
  return typeof v === "string" && (THEMES as readonly string[]).includes(v);
}

/** Lê o tema salvo; qualquer valor desconhecido cai em `light`. */
export function loadTheme(): Theme {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return isTheme(v) ? v : "light";
  } catch {
    return "light";
  }
}

/** Aplica no `<html>` e persiste. */
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    /* localStorage indisponível */
  }
}
