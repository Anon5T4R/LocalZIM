import { t, type MessageKey } from "../lib/i18n";
import { THEMES, type Theme } from "../lib/theme";

interface Props {
  theme: Theme;
  onTheme: (theme: Theme) => void;
  className?: string;
}

/** Seletor de tema — mesmo visual do LocalePicker, usado na toolbar do leitor. */
export default function ThemePicker({ theme, onTheme, className = "" }: Props) {
  return (
    <select
      className={`lang-select ${className}`.trim()}
      value={theme}
      onChange={(e) => onTheme(e.target.value as Theme)}
      title={t("rd.themeTitle")}
      aria-label={t("rd.themeTitle")}
    >
      {THEMES.map((th) => (
        <option key={th} value={th}>
          {t(`theme.${th}` as MessageKey)}
        </option>
      ))}
    </select>
  );
}
