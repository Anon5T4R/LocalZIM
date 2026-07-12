import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ZimInfo } from "../lib/backend";
import { formatBytes } from "../lib/paths";
import { loadRecents, removeRecent, RecentBook } from "../lib/recents";

interface Props {
  books: ZimInfo[];
  error: string | null;
  onOpenPath: (path: string) => void;
  onActivate: (id: string) => void;
  onCloseBook: (id: string) => void;
}

function metaLine(language: string, size: number, articles: number | null): string {
  const parts: string[] = [];
  if (language) parts.push(language);
  if (size > 0) parts.push(formatBytes(size));
  if (articles != null) parts.push(`${articles.toLocaleString("pt-BR")} artigos`);
  return parts.join(" · ");
}

export default function Library({ books, error, onOpenPath, onActivate, onCloseBook }: Props) {
  const [recents, setRecents] = useState<RecentBook[]>(loadRecents);

  const pickFile = async () => {
    const sel = await openDialog({
      title: "Abrir arquivo ZIM",
      multiple: false,
      filters: [{ name: "Arquivos ZIM", extensions: ["zim"] }],
    });
    if (typeof sel === "string") onOpenPath(sel);
  };

  const openPaths = new Set(books.map((b) => b.path));
  const closedRecents = recents.filter((r) => !openPaths.has(r.path));

  return (
    <div className="library">
      <header className="lib-header">
        <div>
          <h1>LocalZIM</h1>
          <p className="lib-sub">
            Sua biblioteca offline — leitor de arquivos ZIM (Wikipédia, Stack Overflow, Gutenberg…)
          </p>
        </div>
        <button className="primary" onClick={pickFile}>
          Abrir arquivo .zim…
        </button>
      </header>

      {error && <div className="error-banner">{error}</div>}

      {books.length > 0 && (
        <section>
          <h2>Abertos</h2>
          <div className="grid">
            {books.map((b) => (
              <div className="card" key={b.id} onClick={() => onActivate(b.id)}>
                {b.favicon ? (
                  <img className="fav" src={b.favicon} alt="" />
                ) : (
                  <div className="fav fav-fallback">📚</div>
                )}
                <div className="card-body">
                  <div className="card-title">{b.name}</div>
                  {b.description && <div className="card-desc">{b.description}</div>}
                  <div className="card-meta">
                    {metaLine(b.language, b.size, b.articleCount)}
                  </div>
                </div>
                <button
                  className="card-x"
                  title="Fechar este arquivo"
                  onClick={(e) => {
                    e.stopPropagation();
                    onCloseBook(b.id);
                  }}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      {closedRecents.length > 0 && (
        <section>
          <h2>Recentes</h2>
          <div className="grid">
            {closedRecents.map((r) => (
              <div className="card" key={r.path} onClick={() => onOpenPath(r.path)}>
                {r.favicon ? (
                  <img className="fav" src={r.favicon} alt="" />
                ) : (
                  <div className="fav fav-fallback">📚</div>
                )}
                <div className="card-body">
                  <div className="card-title">{r.name}</div>
                  {r.description && <div className="card-desc">{r.description}</div>}
                  <div className="card-meta">{metaLine(r.language, r.size, r.articleCount)}</div>
                  <div className="card-path" title={r.path}>
                    {r.path}
                  </div>
                </div>
                <button
                  className="card-x"
                  title="Remover dos recentes"
                  onClick={(e) => {
                    e.stopPropagation();
                    setRecents(removeRecent(r.path));
                  }}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      {books.length === 0 && closedRecents.length === 0 && (
        <div className="empty">
          <div className="empty-icon">📚</div>
          <p>
            Nenhum arquivo aberto ainda. Abra um <strong>.zim</strong> do seu computador — dá para
            baixar a Wikipédia inteira, Wikcionário, Stack Overflow e muito mais em{" "}
            <a
              href="https://library.kiwix.org"
              onClick={(e) => {
                e.preventDefault();
                openUrl("https://library.kiwix.org").catch(() => {});
              }}
            >
              library.kiwix.org
            </a>
            .
          </p>
          <button className="primary" onClick={pickFile}>
            Abrir arquivo .zim…
          </button>
        </div>
      )}
    </div>
  );
}
