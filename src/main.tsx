import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { useLocale } from "./lib/i18n";
import "./styles.css";

// Remonta a árvore ao trocar de idioma → todo t() reavalia.
function Root() {
  const locale = useLocale();
  return <App key={locale} />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
);
