import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  DirectionStatus,
  FtEvent,
  FtHit,
  FtStatus,
  Lang,
  Suggestion,
  TranslateModelEvent,
  ZimInfo,
  fulltextBuild,
  fulltextCancel,
  fulltextSearch,
  fulltextStatus,
  translateCancelDownload,
  translateDownload,
  translatePrepare,
  translateRemove,
  translateStatus,
  translateTexts,
  zimRandom,
  zimSuggest,
} from "../lib/backend";
import { LANG_NAMES, LEG_NAMES, guessLang } from "../lib/lang";
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

/** Blocos por chamada ao backend — pequeno o bastante pra progresso fluido. */
const TR_BATCH = 8;

const fmtMB = (bytes: number) => `${Math.max(1, Math.round(bytes / 1e6))} MB`;

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

  // tradução offline
  const [trOpen, setTrOpen] = useState(false);
  const [trTgt, setTrTgt] = useState<Lang>(() => {
    const v = localStorage.getItem("localzim.translate.tgt");
    return v === "pt" || v === "es" || v === "en" ? v : "pt";
  });
  const [trSrcOverride, setTrSrcOverride] = useState<Lang | "auto">("auto");
  const [pageLang, setPageLang] = useState("");
  const [trStatus, setTrStatus] = useState<DirectionStatus | null>(null);
  const [trPhase, setTrPhase] = useState<"idle" | "loading" | "translating" | "done">("idle");
  const [trProgress, setTrProgress] = useState({ done: 0, total: 0 });
  const [trShowOrig, setTrShowOrig] = useState(false);
  const [trAuto, setTrAuto] = useState(() => localStorage.getItem("localzim.translate.auto") === "1");
  const [trError, setTrError] = useState<string | null>(null);
  const [trDl, setTrDl] = useState<Record<string, { received: number; total: number }>>({});
  const trRun = useRef(0);
  const trBlocksResolve = useRef<((texts: string[]) => void) | null>(null);
  const currentPath = useRef<string>("");

  const trSrc: Lang | null =
    trSrcOverride !== "auto" ? trSrcOverride : guessLang(pageLang) ?? guessLang(active.language);
  const trDirection = trSrc && trSrc !== trTgt ? `${trSrc}-${trTgt}` : null;
  const trDirectionRef = useRef(trDirection);
  trDirectionRef.current = trDirection;
  const trAutoRef = useRef(trAuto);
  trAutoRef.current = trAuto;
  const trStatusRef = useRef(trStatus);
  trStatusRef.current = trStatus;

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

  // ---------- tradução ----------

  const refreshTrStatus = async (direction: string | null) => {
    if (!direction) {
      setTrStatus(null);
      return;
    }
    const st = await translateStatus(direction).catch(() => null);
    setTrStatus(st);
  };

  /** Pede os blocos de texto da página pra ponte e espera a resposta. */
  const collectBlocks = () =>
    new Promise<string[]>((resolve, reject) => {
      trBlocksResolve.current = resolve;
      postToFrame({ type: "zim:collect" });
      setTimeout(() => {
        if (trBlocksResolve.current === resolve) {
          trBlocksResolve.current = null;
          reject(new Error("a página não respondeu"));
        }
      }, 4000);
    });

  const startTranslate = async () => {
    const direction = trDirectionRef.current;
    if (!direction) return;
    const run = ++trRun.current;
    setTrError(null);
    setTrShowOrig(false);
    setTrPhase("loading");
    try {
      await translatePrepare(direction);
      if (run !== trRun.current) return;
      const texts = await collectBlocks();
      if (run !== trRun.current) return;
      if (texts.length === 0) {
        setTrPhase("done");
        setTrProgress({ done: 0, total: 0 });
        return;
      }
      setTrPhase("translating");
      setTrProgress({ done: 0, total: texts.length });
      const article = currentPath.current;
      for (let i = 0; i < texts.length; i += TR_BATCH) {
        if (run !== trRun.current) return;
        const out = await translateTexts(active.id, article, direction, texts.slice(i, i + TR_BATCH));
        if (run !== trRun.current) return;
        postToFrame({ type: "zim:apply", from: i, texts: out });
        setTrProgress({ done: Math.min(i + TR_BATCH, texts.length), total: texts.length });
      }
      setTrPhase("done");
    } catch (e) {
      if (run === trRun.current) {
        setTrError(String(e));
        setTrPhase("idle");
      }
    }
  };
  const startTranslateRef = useRef(startTranslate);
  startTranslateRef.current = startTranslate;

  const cancelTranslate = () => {
    trRun.current += 1;
    setTrPhase("idle");
  };

  const toggleOriginal = () => {
    const on = !trShowOrig;
    setTrShowOrig(on);
    postToFrame({ type: "zim:original", on });
  };

  const downloadMissing = () => {
    setTrError(null);
    for (const l of trStatus?.legs ?? []) {
      if (!l.installed && !l.downloading) translateDownload(l.leg).catch((e) => setTrError(String(e)));
    }
  };

  const removeLeg = async (leg: string) => {
    await translateRemove(leg).catch(() => {});
    refreshTrStatus(trDirectionRef.current);
  };

  useEffect(() => {
    localStorage.setItem("localzim.translate.tgt", trTgt);
  }, [trTgt]);
  useEffect(() => {
    localStorage.setItem("localzim.translate.auto", trAuto ? "1" : "0");
  }, [trAuto]);

  // status dos modelos acompanha a direção (e o painel aberto)
  useEffect(() => {
    if (trOpen) refreshTrStatus(trDirection);
  }, [trOpen, trDirection]);

  // progresso de download dos modelos
  useEffect(() => {
    const un = listen<TranslateModelEvent>("translate-model", (e) => {
      const p = e.payload;
      if (p.state === "downloading") {
        setTrDl((m) => ({ ...m, [p.leg]: { received: p.received ?? 0, total: p.total ?? 0 } }));
        return;
      }
      setTrDl((m) => {
        const n = { ...m };
        delete n[p.leg];
        return n;
      });
      if (p.state === "error" && p.error) setTrError(p.error);
      refreshTrStatus(trDirectionRef.current);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

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
    trRun.current += 1;
    setTrOpen(false);
    setTrPhase("idle");
    setTrShowOrig(false);
    setTrError(null);
    setTrSrcOverride("auto");
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
        lang?: string;
        texts?: string[];
      };
      if (d.type === "zim:loaded") {
        const parsed = pathFromHref(String(d.href ?? ""));
        const t = String(d.title ?? "");
        setTitle(t);
        if (parsed) {
          currentPath.current = parsed.path;
          onLoadedRef.current(parsed.id, parsed.path, t);
        }
        const w = iframeRef.current?.contentWindow;
        if (w) {
          w.postMessage({ type: "zim:zoom", value: String(zoomRef.current / 100) }, "*");
          w.postMessage({ type: "zim:dark", on: darkRef.current }, "*");
        }
        getCurrentWindow()
          .setTitle(t ? `${t} — LocalZIM` : "LocalZIM")
          .catch(() => {});
        // página nova: estado de tradução recomeça do zero
        setPageLang(String(d.lang ?? ""));
        trRun.current += 1;
        setTrPhase("idle");
        setTrShowOrig(false);
        setTrError(null);
        if (
          trAutoRef.current &&
          trDirectionRef.current &&
          trStatusRef.current?.legs.every((l) => l.installed)
        ) {
          startTranslateRef.current();
        }
      } else if (d.type === "zim:blocks") {
        trBlocksResolve.current?.(Array.isArray(d.texts) ? d.texts.map(String) : []);
        trBlocksResolve.current = null;
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
        <button
          className={trOpen || trPhase === "done" ? "tb tb-on" : "tb"}
          onClick={() => setTrOpen((o) => !o)}
          title="Traduzir página (offline)"
        >
          🌐
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

        {trOpen && (
          <div className="ftpanel trpanel">
            <div className="ft-head">
              <div className="ft-title">Tradução offline</div>
              <button className="tb" onClick={() => setTrOpen(false)} title="Fechar">
                ✕
              </button>
            </div>

            <div className="tr-langs">
              <span className="tr-label">Traduzir para</span>
              <div className="tr-seg">
                {(Object.keys(LANG_NAMES) as Lang[]).map((l) => (
                  <button
                    key={l}
                    className={trTgt === l ? "seg sel" : "seg"}
                    onClick={() => setTrTgt(l)}
                  >
                    {LANG_NAMES[l]}
                  </button>
                ))}
              </div>
            </div>

            <div className="tr-langs">
              <span className="tr-label">Idioma do artigo</span>
              <select
                className="tr-src"
                value={trSrcOverride}
                onChange={(e) => setTrSrcOverride(e.target.value as Lang | "auto")}
              >
                <option value="auto">
                  {trSrcOverride === "auto" && trSrc
                    ? `Detectado: ${LANG_NAMES[trSrc]}`
                    : "Detectar automaticamente"}
                </option>
                {(Object.keys(LANG_NAMES) as Lang[]).map((l) => (
                  <option key={l} value={l}>
                    {LANG_NAMES[l]}
                  </option>
                ))}
              </select>
            </div>

            {!trSrc && (
              <div className="ft-block">
                Não deu pra detectar o idioma deste artigo — escolha acima. A tradução
                funciona entre português, espanhol e inglês.
              </div>
            )}
            {trSrc && trSrc === trTgt && (
              <div className="ft-block">O artigo já está em {LANG_NAMES[trTgt]}.</div>
            )}

            {trDirection && trStatus && !trStatus.legs.every((l) => l.installed) && (
              <div className="ft-block">
                <p>
                  Primeira vez nesta direção: o LocalZIM baixa o modelo de tradução{" "}
                  <strong>uma única vez</strong> e depois funciona 100% offline.
                  {trStatus.legs.length > 1 && (
                    <> Português ↔ espanhol passa pelo inglês, então são dois modelos.</>
                  )}
                </p>
                {trStatus.legs.filter((l) => !l.installed).map((l) => {
                  const dl = trDl[l.leg];
                  return (
                    <div key={l.leg} className="tr-leg">
                      <span className="tr-leg-name">
                        {LEG_NAMES[l.leg] ?? l.leg} · {fmtMB(l.bytes)}
                      </span>
                      {dl ? (
                        <>
                          <div className="ft-progress">
                            <div
                              style={{
                                width: `${dl.total ? (dl.received / dl.total) * 100 : 0}%`,
                              }}
                            />
                          </div>
                          <button
                            className="ghost"
                            onClick={() => translateCancelDownload(l.leg)}
                          >
                            Cancelar
                          </button>
                        </>
                      ) : l.downloading ? (
                        <span className="tr-muted">preparando…</span>
                      ) : null}
                    </div>
                  );
                })}
                {!trStatus.legs.some((l) => l.downloading || trDl[l.leg]) && (
                  <button className="primary" onClick={downloadMissing}>
                    Baixar {trStatus.legs.filter((l) => !l.installed).length > 1 ? "modelos" : "modelo"}{" "}
                    ({fmtMB(
                      trStatus.legs.filter((l) => !l.installed).reduce((s, l) => s + l.bytes, 0)
                    )})
                  </button>
                )}
              </div>
            )}

            {trDirection && trStatus?.legs.every((l) => l.installed) && (
              <div className="ft-block">
                {trPhase === "idle" && (
                  <button className="primary" onClick={() => startTranslateRef.current()}>
                    Traduzir página
                  </button>
                )}
                {trPhase === "loading" && <p>Carregando modelo…</p>}
                {trPhase === "translating" && (
                  <>
                    <p>
                      Traduzindo… {trProgress.done}/{trProgress.total} blocos
                    </p>
                    <div className="ft-progress">
                      <div
                        style={{
                          width: `${trProgress.total ? (trProgress.done / trProgress.total) * 100 : 0}%`,
                        }}
                      />
                    </div>
                    <button className="ghost" onClick={cancelTranslate}>
                      Parar
                    </button>
                  </>
                )}
                {trPhase === "done" && (
                  <>
                    <p>✓ Página traduzida (fica em cache — voltar aqui é instantâneo).</p>
                    <button className="ghost" onClick={toggleOriginal}>
                      {trShowOrig ? "Ver tradução" : "Ver original"}
                    </button>
                  </>
                )}
                <label className="tr-auto">
                  <input
                    type="checkbox"
                    checked={trAuto}
                    onChange={(e) => setTrAuto(e.target.checked)}
                  />
                  Traduzir as próximas páginas automaticamente
                </label>
                <div className="tr-manage">
                  {trStatus.legs.map((l) => (
                    <button
                      key={l.leg}
                      className="tr-remove"
                      title={`Apagar o modelo ${LEG_NAMES[l.leg] ?? l.leg} do disco (${fmtMB(l.bytes)})`}
                      onClick={() => removeLeg(l.leg)}
                    >
                      🗑 {LEG_NAMES[l.leg] ?? l.leg}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {trError && <div className="ft-block tr-error">{trError}</div>}
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
