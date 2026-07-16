import { LOCALE_LABELS, setLocale, t, useLocale, type Locale } from "../lib/i18n";

/** Seletor de idioma (EN/PT/ES) — usado no header da biblioteca e na toolbar. */
export default function LocalePicker({ className = "" }: { className?: string }) {
  const locale = useLocale();
  return (
    <select
      className={`lang-select ${className}`.trim()}
      value={locale}
      onChange={(e) => setLocale(e.target.value as Locale)}
      title={t("lang.title")}
      aria-label={t("lang.title")}
    >
      {(Object.keys(LOCALE_LABELS) as Locale[]).map((l) => (
        <option key={l} value={l}>
          {LOCALE_LABELS[l]}
        </option>
      ))}
    </select>
  );
}
