import { Lang } from "./backend";

/** Nomes exibidos dos idiomas de tradução. */
export const LANG_NAMES: Record<Lang, string> = {
  pt: "Português",
  es: "Español",
  en: "English",
};

/** Modelos (pernas) de tradução, na nomenclatura das releases. */
export const LEG_NAMES: Record<string, string> = {
  "en-pt": "inglês → português",
  "pt-en": "português → inglês",
  "en-es": "inglês → espanhol",
  "es-en": "espanhol → inglês",
};

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
