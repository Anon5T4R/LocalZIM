import { Lang } from "./backend";

/** Nomes exibidos dos idiomas de tradução. */
export const LANG_NAMES: Record<Lang, string> = {
  pt: "Português",
  es: "Español",
  en: "English",
};

/**
 * Nome de um modelo (perna) de tradução, ex.: "en-pt" → "English → Português".
 * Montado a partir dos endônimos (LANG_NAMES), então é neutro de idioma da UI.
 */
export function legName(leg: string): string {
  const [a, b] = leg.split("-") as Lang[];
  return LANG_NAMES[a] && LANG_NAMES[b] ? `${LANG_NAMES[a]} → ${LANG_NAMES[b]}` : leg;
}

/**
 * Detecta um idioma suportado a partir do metadado do livro ou do atributo
 * lang do artigo: "eng", "pt-BR", "spa;por", "en_US"… → "en" | "pt" | "es".
 */
export function guessLang(raw: string): Lang | null {
  const v = raw.toLowerCase().split(/[-_,;\s]/)[0];
  if (v === "en" || v === "eng") return "en";
  if (v === "pt" || v === "por" || v === "pob") return "pt";
  if (v === "es" || v === "spa") return "es";
  return null;
}
