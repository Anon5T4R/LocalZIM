import { useEffect, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import {
  cancelCreateZim,
  createZim,
  createZimFromSite,
  ZimCreateEvent,
  ZimInfo,
} from "../lib/backend";
import { formatBytes } from "../lib/paths";
import { loadRecents, removeRecent, RecentBook } from "../lib/recents";

interface Props {
  books: ZimInfo[];
  error: string | null;
  onOpenPath: (path: string) => void;
  onActivate: (id: string) => void;
  onCloseBook: (id: string) => void;
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
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

  // criador de .zim a partir de pasta
  const [createOpen, setCreateOpen] = useState(false);
  const [cSource, setCSource] = useState("");
  const [cOutput, setCOutput] = useState("");
  const [cTitle, setCTitle] = useState("");
  const [cDesc, setCDesc] = useState("");
  const [cLang, setCLang] = useState("por");
  const [cCreator, setCCreator] = useState("");
  const [cMain, setCMain] = useState("");
  const [cState, setCState] = useState<ZimCreateEvent | null>(null);

  useEffect(() => {
    const un = listen<ZimCreateEvent>("zim-create", (e) => {
      setCState(e.payload);
      if (e.payload.state === "done" && e.payload.result) {
        onOpenPath(e.payload.result.output);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [onOpenPath]);

  const pickFile = async () => {
    const sel = await openDialog({
      title: "Abrir arquivo ZIM",
      multiple: false,
      filters: [{ name: "Arquivos ZIM", extensions: ["zim"] }],
    });
    if (typeof sel === "string") onOpenPath(sel);
  };

  const pickSourceDir = async () => {
    const sel = await openDialog({ title: "Pasta com o conteúdo (HTML)", directory: true });
    if (typeof sel === "string") {
      setCSource(sel);
      if (!cTitle) {
        const name = sel.split(/[\\/]/).filter(Boolean).pop() ?? "";
        setCTitle(name);
      }
    }
  };

  const pickOutput = async () => {
    const sel = await saveDialog({
      title: "Salvar arquivo ZIM",
      defaultPath: `${(cTitle || "biblioteca").replace(/[\\/:*?"<>|]/g, "-")}.zim`,
      filters: [{ name: "Arquivo ZIM", extensions: ["zim"] }],
    });
    if (typeof sel === "string") setCOutput(sel);
  };

  const startCreate = async () => {
    setCState({ state: "building", progress: 0 });
    try {
      await createZim({
        source: cSource,
        output: cOutput,
        title: cTitle.trim() || "Biblioteca",
        description: cDesc,
        language: cLang,
        creator: cCreator,
        mainPage: cMain.trim() || null,
      });
    } catch (e) {
      setCState({ state: "error", progress: 0, error: String(e) });
    }
  };

  // criador a partir de um site (crawler local)
  const [siteOpen, setSiteOpen] = useState(false);
  const [sUrl, setSUrl] = useState("");
  const [sOutput, setSOutput] = useState("");
  const [sTitle, setSTitle] = useState("");
  const [sDesc, setSDesc] = useState("");
  const [sLang, setSLang] = useState("por");
  const [sDepth, setSDepth] = useState("3");
  const [sMaxPages, setSMaxPages] = useState("200");

  const pickSiteOutput = async () => {
    const sel = await saveDialog({
      title: "Salvar arquivo ZIM",
      defaultPath: `${(sTitle || "site").replace(/[\\/:*?"<>|]/g, "-")}.zim`,
      filters: [{ name: "Arquivo ZIM", extensions: ["zim"] }],
    });
    if (typeof sel === "string") setSOutput(sel);
  };

  const startSite = async () => {
    setCState({ state: "building", progress: 0, phase: "crawl", pages: 0 });
    try {
      await createZimFromSite({
        url: sUrl.trim(),
        output: sOutput,
        title: sTitle.trim() || hostOf(sUrl) || "Site",
        description: sDesc,
        language: sLang,
        maxDepth: Math.max(0, parseInt(sDepth, 10) || 3),
        maxPages: Math.max(1, parseInt(sMaxPages, 10) || 200),
      });
    } catch (e) {
      setCState({ state: "error", progress: 0, error: String(e) });
    }
  };

  const building = cState?.state === "building";

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
        <div className="lib-actions">
          <button className="primary" onClick={pickFile}>
            Abrir arquivo .zim…
          </button>
          <button
            className="secondary"
            onClick={() => {
              setCState(null);
              setCreateOpen(true);
            }}
            title="Empacota uma pasta com HTML num arquivo .zim"
          >
            Criar .zim de uma pasta…
          </button>
          <button
            className="secondary"
            onClick={() => {
              setCState(null);
              setSiteOpen(true);
            }}
            title="Baixa um site (crawler local) e empacota num .zim"
          >
            Criar .zim de um site…
          </button>
        </div>
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

      {siteOpen && (
        <div className="modal-overlay" onClick={() => !building && setSiteOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>Criar .zim de um site</h3>
            <p className="modal-hint">
              Crawler local: baixa as páginas do <strong>mesmo domínio</strong> (respeitando o
              robots.txt) com imagens, CSS e scripts, reescreve os links e empacota. Funciona bem
              pra documentação, blogs e wikis; sites montados por JavaScript (SPA) podem sair
              incompletos — pra esses, use o{" "}
              <a
                href="https://github.com/openzim/zimit"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl("https://github.com/openzim/zimit").catch(() => {});
                }}
              >
                zimit
              </a>
              .
            </p>

            <div className="form-row">
              <label>Endereço do site</label>
              <input
                value={sUrl}
                onChange={(e) => {
                  setSUrl(e.target.value);
                  if (!sTitle) setSTitle(hostOf(e.target.value));
                }}
                disabled={building}
                placeholder="https://docs.exemplo.com"
                spellCheck={false}
              />
            </div>
            <div className="form-row">
              <label>Salvar como</label>
              <div className="form-pick">
                <input value={sOutput} readOnly placeholder="destino do arquivo .zim" />
                <button onClick={pickSiteOutput} disabled={building}>
                  Escolher…
                </button>
              </div>
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>Título</label>
                <input value={sTitle} onChange={(e) => setSTitle(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>Idioma</label>
                <input value={sLang} onChange={(e) => setSLang(e.target.value)} disabled={building} />
              </div>
            </div>
            <div className="form-row">
              <label>Descrição</label>
              <input value={sDesc} onChange={(e) => setSDesc(e.target.value)} disabled={building} />
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>Profundidade de links</label>
                <input value={sDepth} onChange={(e) => setSDepth(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>Máximo de páginas</label>
                <input
                  value={sMaxPages}
                  onChange={(e) => setSMaxPages(e.target.value)}
                  disabled={building}
                />
              </div>
            </div>

            {building && (
              <div className="ft-block" style={{ padding: "10px 0 0" }}>
                <p style={{ margin: "0 0 6px" }}>
                  {cState?.phase === "crawl"
                    ? `Baixando páginas… ${cState?.pages ?? 0} (fila: ${cState?.queued ?? 0})`
                    : `Empacotando… ${Math.round((cState?.progress ?? 0) * 100)}%`}
                </p>
                <div className="ft-progress">
                  <div style={{ width: `${(cState?.progress ?? 0) * 100}%` }} />
                </div>
              </div>
            )}
            {cState?.state === "error" && <div className="error-banner">{cState.error}</div>}
            {cState?.state === "done" && cState.result && (
              <div className="ok-banner">
                Pronto: {cState.result.articles} páginas, {formatBytes(cState.result.size)} — o
                arquivo já foi aberto na biblioteca.
              </div>
            )}

            <div className="modal-actions">
              {!building && (
                <button className="primary" disabled={!sUrl.trim() || !sOutput} onClick={startSite}>
                  Baixar e criar
                </button>
              )}
              {building && (
                <button className="ghost" onClick={() => cancelCreateZim()}>
                  Cancelar
                </button>
              )}
              <button disabled={building} onClick={() => setSiteOpen(false)}>
                Fechar
              </button>
            </div>
          </div>
        </div>
      )}

      {createOpen && (
        <div className="modal-overlay" onClick={() => !building && setCreateOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>Criar .zim de uma pasta</h3>
            <p className="modal-hint">
              Empacota uma pasta com HTML (site salvo, documentação, notas exportadas) num
              arquivo <code>.zim</code>. Links relativos entre as páginas continuam funcionando.
              Para capturar um site da internet, use o{" "}
              <a
                href="https://github.com/openzim/zimit"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl("https://github.com/openzim/zimit").catch(() => {});
                }}
              >
                zimit
              </a>{" "}
              (ou <code>wget --mirror</code> e empacote aqui).
            </p>

            <div className="form-row">
              <label>Pasta de origem</label>
              <div className="form-pick">
                <input value={cSource} readOnly placeholder="escolha a pasta com o conteúdo" />
                <button onClick={pickSourceDir} disabled={building}>
                  Escolher…
                </button>
              </div>
            </div>
            <div className="form-row">
              <label>Salvar como</label>
              <div className="form-pick">
                <input value={cOutput} readOnly placeholder="destino do arquivo .zim" />
                <button onClick={pickOutput} disabled={building}>
                  Escolher…
                </button>
              </div>
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>Título</label>
                <input value={cTitle} onChange={(e) => setCTitle(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>Idioma</label>
                <input value={cLang} onChange={(e) => setCLang(e.target.value)} disabled={building} placeholder="por" />
              </div>
            </div>
            <div className="form-row">
              <label>Descrição</label>
              <input value={cDesc} onChange={(e) => setCDesc(e.target.value)} disabled={building} />
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>Criador</label>
                <input value={cCreator} onChange={(e) => setCCreator(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>Página inicial</label>
                <input
                  value={cMain}
                  onChange={(e) => setCMain(e.target.value)}
                  disabled={building}
                  placeholder="auto (index.html)"
                />
              </div>
            </div>

            {building && (
              <div className="ft-block" style={{ padding: "10px 0 0" }}>
                <p style={{ margin: "0 0 6px" }}>
                  Empacotando… {Math.round((cState?.progress ?? 0) * 100)}%
                </p>
                <div className="ft-progress">
                  <div style={{ width: `${(cState?.progress ?? 0) * 100}%` }} />
                </div>
              </div>
            )}
            {cState?.state === "error" && <div className="error-banner">{cState.error}</div>}
            {cState?.state === "done" && cState.result && (
              <div className="ok-banner">
                Pronto: {cState.result.articles} artigos, {formatBytes(cState.result.size)} — o
                arquivo já foi aberto na biblioteca.
              </div>
            )}

            <div className="modal-actions">
              {!building && (
                <button
                  className="primary"
                  disabled={!cSource || !cOutput}
                  onClick={startCreate}
                >
                  Criar
                </button>
              )}
              {building && (
                <button className="ghost" onClick={() => cancelCreateZim()}>
                  Cancelar
                </button>
              )}
              <button disabled={building} onClick={() => setCreateOpen(false)}>
                Fechar
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
