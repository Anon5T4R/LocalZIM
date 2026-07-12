import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  FtEvent,
  FtHit,
  FtStatus,
  Suggestion,
  ZimInfo,
  fulltextBuild,
  fulltextCancel,
  fulltextSearch,
  fulltextStatus,
  zimRandom,
  zimSuggest,
} from "../lib/backend";
import { pathFromHref, zimUrl } from "../lib/paths";

export interface NavTarget {
  id: string;
  path: string;
  n: number;
}

interface Props {
  active: ZimInfo;
  nav: NavTarget | null;
  dark: boolean;
  onToggleDark: () => void;
  onLibrary: () => void;
  onLoaded: (id: string, path: string, title: string) => void;
}

const ZOOM_MIN = 50;
const ZOOM_MAX = 300;

export default function Reader({ active, nav, dark, onToggleDark, onLibrary, onLoaded }: Props) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [title, setTitle] = useState("");
  const [zoom, setZoom] = useState(100);
  const zoomMap = useRef(new Map<string, number>());
  const zoomRef = useRef(100);
  const darkRef = useRef(dark);
  darkRef.current = dark;
  const onLoadedRef = useRef(onLoaded);
  onLoadedRef.current = onLoaded;
  const activeIdRef = useRef(active.id);
  activeIdRef.current = active.id;

  const [query, setQuery] = useState("");
  const [sugs, setSugs] = useState<Suggestion[]>([]);
  const [showSugs, setShowSugs] = useState(false);
  const [sel, setSel] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);

  // localizar na página (Ctrl+F)
  const [findOpen, setFindOpen] = useState(false);
  const [findQ, setFindQ] = useState("");
  const findRef = useRef<HTMLInputElement>(null);

  // busca no texto completo
  const [ftOpen, setFtOpen] = useState(false);
  const [ftQuery, setFtQuery] = useState("");
  const [ftStatus, setFtStatus] = useState<FtStatus | null>(null);
  const [ftResults, setFtResults] = useState<FtHit[] | null>(null);
  const [ftBusy, setFtBusy] = useState(false);
  const ftQueryRef = useRef("");
  ftQueryRef.current = ftQuery;

  const postToFrame = (msg: unknown) => {
    iframeRef.current?.contentWindow?.postMessage(msg, "*");
  };

  const navigate = (path: string) => {
    const el = iframeRef.current;
    if (el) el.src = zimUrl(active.id, path);
    setShowSugs(false);
    setQuery("");
  };

  const applyZoom = (v: number) => {
    const c = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, v));
    zoomMap.current.set(active.id, c);
    zoomRef.current = c;
    setZoom(c);
    postToFrame({ type: "zim:zoom", value: String(c / 100) });
  };

  const doFind = (prev: boolean) => {
    if (findQ.trim()) postToFrame({ type: "zim:find", q: findQ, prev });
  };

  const runFtSearch = async (q: string) => {
    setFtBusy(true);
    try {
      setFtResults(await fulltextSearch(active.id, q, 30));
    } catch {
      setFtResults([]);
    }
    setFtBusy(false);
  };
  const runFtSearchRef = useRef(runFtSearch);
  runFtSearchRef.current = runFtSearch;

  const openFulltext = async (q: string) => {
    const qq = q.trim();
    if (!qq) return;
    setFtOpen(true);
    setFtQuery(qq);
    setFtResults(null);
    setShowSugs(false);
    const st = await fulltextStatus(active.id).catch(() => null);
    setFtStatus(st);
    if (st?.state === "ready") runFtSearch(qq);
  };

  // ações dos atalhos — vindas da ponte no artigo ou do teclado no app
  const keyAction = (k: string) => {
    if (k === "back") history.back();
    else if (k === "forward") history.forward();
    else if (k === "zoomin") applyZoom(zoomRef.current + 10);
    else if (k === "zoomout") applyZoom(zoomRef.current - 10);
    else if (k === "zoomreset") applyZoom(100);
    else if (k === "search") {
      searchRef.current?.focus();
      searchRef.current?.select();
    } else if (k === "find") {
      setFindOpen(true);
      setTimeout(() => {
        findRef.current?.focus();
        findRef.current?.select();
      }, 0);
    }
  };
  const keyActionRef = useRef(keyAction);
  keyActionRef.current = keyAction;

  // navegação imperativa vinda do App (abrir livro, trocar de livro)
  useEffect(() => {
    if (!nav) return;
    const el = iframeRef.current;
    if (el) el.src = zimUrl(nav.id, nav.path);
  }, [nav]);

  // preferências e painéis por livro
  useEffect(() => {
    const z = zoomMap.current.get(active.id) ?? 100;
    zoomRef.current = z;
    setZoom(z);
    setFtOpen(false);
    setFtStatus(null);
    setFtResults(null);
    setFtQuery("");
    setFindOpen(false);
  }, [active.id]);

  // tema do artigo acompanha o tema do app
  useEffect(() => {
    postToFrame({ type: "zim:dark", on: dark });
  }, [dark]);

  // mensagens da ponte injetada nas páginas
  useEffect(() => {
    const onMsg = (ev: MessageEvent) => {
      const d = (ev.data ?? {}) as {
        type?: string;
        href?: string;
        title?: string;
        url?: string;
        key?: string;
      };
      if (d.type === "zim:loaded") {
        const parsed = pathFromHref(String(d.href ?? ""));
        const t = String(d.title ?? "");
        setTitle(t);
        if (parsed) onLoadedRef.current(parsed.id, parsed.path, t);
        const w = iframeRef.current?.contentWindow;
        if (w) {
          w.postMessage({ type: "zim:zoom", value: String(zoomRef.current / 100) }, "*");
          w.postMessage({ type: "zim:dark", on: darkRef.current }, "*");
        }
        getCurrentWindow()
          .setTitle(t ? `${t} — LocalZIM` : "LocalZIM")
          .catch(() => {});
      } else if (d.type === "zim:external" && d.url) {
        openUrl(String(d.url)).catch(() => {});
      } else if (d.type === "zim:key" && d.key) {
        keyActionRef.current(String(d.key));
      }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, []);

  // mesmos atalhos com o foco no próprio app
  useEffect(() => {
    const onKey = (ev: KeyboardEvent) => {
      let k: string | null = null;
      if (ev.altKey && ev.key === "ArrowLeft") k = "back";
      else if (ev.altKey && ev.key === "ArrowRight") k = "forward";
      else if ((ev.ctrlKey || ev.metaKey) && (ev.key === "=" || ev.key === "+")) k = "zoomin";
      else if ((ev.ctrlKey || ev.metaKey) && ev.key === "-") k = "zoomout";
      else if ((ev.ctrlKey || ev.metaKey) && ev.key === "0") k = "zoomreset";
      else if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "k") k = "search";
      else if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "f") k = "find";
      if (k) {
        ev.preventDefault();
        keyActionRef.current(k);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // progresso/estado da indexação full-text
  useEffect(() => {
    const un = listen<FtEvent>("fulltext", (e) => {
      if (e.payload.id !== activeIdRef.current) return;
      setFtStatus({
        state: e.payload.state,
        progress: e.payload.progress,
        docs: e.payload.docs ?? null,
      });
      if (e.payload.state === "ready" && ftQueryRef.current) {
        runFtSearchRef.current(ftQueryRef.current);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // sugestões com debounce
  useEffect(() => {
    if (!query.trim()) {
      setSugs([]);
      setShowSugs(false);
      return;
    }
    const t = setTimeout(() => {
      zimSuggest(active.id, query, 12)
        .then((s) => {
          setSugs(s);
          setSel(0);
          setShowSugs(true);
        })
        .catch(() => setSugs([]));
    }, 150);
    return () => clearTimeout(t);
  }, [query, active.id]);

  const goRandom = async () => {
    const p = await zimRandom(active.id).catch(() => null);
    if (p) navigate(p);
  };

  // itens do dropdown = sugestões + "buscar no texto completo"
  const totalItems = sugs.length + 1;
  const onSearchKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !showSugs) {
      e.preventDefault();
      openFulltext(query);
      return;
    }
    if (!showSugs) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => (s + 1) % totalItems);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => (s - 1 + totalItems) % totalItems);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (sel < sugs.length) navigate(sugs[sel].path);
      else openFulltext(query);
    } else if (e.key === "Escape") {
      setShowSugs(false);
    }
  };

  return (
    <div className="reader">
      <div className="toolbar">
        <button className="tb" onClick={onLibrary} title="Biblioteca">
          ☰
        </button>
        <button className="tb" onClick={() => history.back()} title="Voltar (Alt+←)">
          ←
        </button>
        <button className="tb" onClick={() => history.forward()} title="Avançar (Alt+→)">
          →
        </button>
        <button
          className="tb"
          onClick={() => active.mainPath && navigate(active.mainPath)}
          title="Página principal"
        >
          ⌂
        </button>
        <button className="tb" onClick={goRandom} title="Artigo aleatório">
          🎲
        </button>

        <div className="search">
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKey}
            onFocus={() => query.trim() && setShowSugs(true)}
            onBlur={() => setTimeout(() => setShowSugs(false), 150)}
            placeholder={`Buscar em ${active.name}… (Ctrl+K)`}
            spellCheck={false}
          />
          {showSugs && query.trim() && (
            <ul className="sugs">
              {sugs.map((s, i) => (
                <li
                  key={s.path + i}
                  className={i === sel ? "sel" : ""}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    navigate(s.path);
                  }}
                >
                  {s.title}
                </li>
              ))}
              <li
                className={sel === sugs.length ? "sel ft-item" : "ft-item"}
                onMouseDown={(e) => {
                  e.preventDefault();
                  openFulltext(query);
                }}
              >
                🔎 Buscar “{query.trim()}” no texto completo
              </li>
            </ul>
          )}
        </div>

        <div className="zoom">
          <button className="tb" onClick={() => applyZoom(zoom - 10)} title="Diminuir zoom (Ctrl+-)">
            −
          </button>
          <span className="zoom-label">{zoom}%</span>
          <button className="tb" onClick={() => applyZoom(zoom + 10)} title="Aumentar zoom (Ctrl+=)">
            +
          </button>
        </div>

        <button
          className="tb"
          onClick={() => keyActionRef.current("find")}
          title="Localizar na página (Ctrl+F)"
        >
          🔍
        </button>
        <button className="tb" onClick={onToggleDark} title="Alternar tema claro/escuro">
          {dark ? "☀️" : "🌙"}
        </button>

        <div className="crumb" title={title}>
          <span className="crumb-book">{active.name}</span>
          {title && <span className="crumb-title"> · {title}</span>}
        </div>
      </div>

      <div className="reader-body">
        <iframe ref={iframeRef} className="content" title="Conteúdo do arquivo ZIM" />

        {findOpen && (
          <div className="findbar">
            <input
              ref={findRef}
              value={findQ}
              onChange={(e) => setFindQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") doFind(e.shiftKey);
                else if (e.key === "Escape") setFindOpen(false);
              }}
              placeholder="Localizar na página…"
              spellCheck={false}
            />
            <button className="tb" onClick={() => doFind(true)} title="Anterior (Shift+Enter)">
              ↑
            </button>
            <button className="tb" onClick={() => doFind(false)} title="Próximo (Enter)">
              ↓
            </button>
            <button className="tb" onClick={() => setFindOpen(false)} title="Fechar (Esc)">
              ✕
            </button>
          </div>
        )}

        {ftOpen && (
          <div className="ftpanel">
            <div className="ft-head">
              <div className="ft-title">Texto completo</div>
              <button className="tb" onClick={() => setFtOpen(false)} title="Fechar">
                ✕
              </button>
            </div>
            <div className="ft-query">“{ftQuery}”</div>

            {ftStatus?.state === "none" && (
              <div className="ft-block">
                <p>
                  Este arquivo ainda não tem índice de busca. O LocalZIM cria um índice local{" "}
                  <strong>uma única vez</strong> — pode demorar e ocupar espaço em disco,
                  proporcional ao tamanho do arquivo.
                </p>
                <button className="primary" onClick={() => fulltextBuild(active.id)}>
                  Criar índice agora
                </button>
              </div>
            )}

            {ftStatus?.state === "building" && (
              <div className="ft-block">
                <p>Indexando artigos… {Math.round((ftStatus.progress ?? 0) * 100)}%</p>
                <div className="ft-progress">
                  <div style={{ width: `${(ftStatus.progress ?? 0) * 100}%` }} />
                </div>
                <button className="ghost" onClick={() => fulltextCancel(active.id)}>
                  Cancelar
                </button>
              </div>
            )}

            {ftStatus?.state === "error" && (
              <div className="ft-block">Falha na indexação — tente de novo.</div>
            )}

            {ftStatus?.state === "ready" && (
              <div className="ft-results">
                {ftBusy && <div className="ft-block">Buscando…</div>}
                {!ftBusy && ftResults?.length === 0 && (
                  <div className="ft-block">Nada encontrado.</div>
                )}
                {!ftBusy &&
                  ftResults?.map((r) => (
                    <div key={r.path} className="ft-hit" onClick={() => navigate(r.path)}>
                      <div className="ft-hit-title">{r.title}</div>
                      <div
                        className="ft-hit-snippet"
                        dangerouslySetInnerHTML={{ __html: r.snippet }}
                      />
                    </div>
                  ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
