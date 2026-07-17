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
import { LANG_NAMES, legName, guessLang } from "../lib/lang";
import { pathFromHref, zimUrl } from "../lib/paths";
import { t } from "../lib/i18n";
import LocalePicker from "./LocalePicker";
import ThemePicker from "./ThemePicker";
import { type Theme } from "../lib/theme";

export interface NavTarget {
  id: string;
  path: string;
  n: number;
}

interface Props {
  active: ZimInfo;
  nav: NavTarget | null;
  theme: Theme;
  /** Modo escuro derivado do tema — é só isso que o artigo (iframe) recebe. */
  dark: boolean;
  onTheme: (theme: Theme) => void;
  onLibrary: () => void;
  onLoaded: (id: string, path: string, title: string) => void;
}

const ZOOM_MIN = 50;
const ZOOM_MAX = 300;

/** Blocos por chamada ao backend — pequeno o bastante pra progresso fluido. */
const TR_BATCH = 8;

const fmtMB = (bytes: number) => `${Math.max(1, Math.round(bytes / 1e6))} MB`;

/** Host de uma URL pra exibir em destaque no aviso (ex.: doc.rust-lang.org). */
function urlHost(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

export default function Reader({ active, nav, theme, dark, onTheme, onLibrary, onLoaded }: Props) {
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

  // link pra fora do arquivo: confirma antes de mandar pro navegador
  const [extUrl, setExtUrl] = useState<string | null>(null);
  const skipExtWarn = useRef(false);

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
          reject(new Error(t("tr.pageNoResponse")));
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

  // Esc fecha o aviso de link externo
  useEffect(() => {
    if (!extUrl) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setExtUrl(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [extUrl]);

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
        const url = String(d.url);
        if (skipExtWarn.current) openUrl(url).catch(() => {});
        else setExtUrl(url);
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
        <button className="tb" onClick={onLibrary} title={t("rd.library")}>
          ☰
        </button>
        <button className="tb" onClick={() => history.back()} title={t("rd.back")}>
          ←
        </button>
        <button className="tb" onClick={() => history.forward()} title={t("rd.forward")}>
          →
        </button>
        <button
          className="tb"
          onClick={() => active.mainPath && navigate(active.mainPath)}
          title={t("rd.home")}
        >
          ⌂
        </button>
        <button className="tb" onClick={goRandom} title={t("rd.random")}>
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
            placeholder={t("rd.searchPlaceholder", { name: active.name })}
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
                🔎 {t("rd.searchFt", { q: query.trim() })}
              </li>
            </ul>
          )}
        </div>

        <div className="zoom">
          <button className="tb" onClick={() => applyZoom(zoom - 10)} title={t("rd.zoomOut")}>
            −
          </button>
          <span className="zoom-label">{zoom}%</span>
          <button className="tb" onClick={() => applyZoom(zoom + 10)} title={t("rd.zoomIn")}>
            +
          </button>
        </div>

        <button
          className="tb"
          onClick={() => keyActionRef.current("find")}
          title={t("rd.findTitle")}
        >
          🔍
        </button>
        <button
          className={trOpen || trPhase === "done" ? "tb tb-on" : "tb"}
          onClick={() => setTrOpen((o) => !o)}
          title={t("rd.translateTitle")}
        >
          🌐
        </button>
        <ThemePicker theme={theme} onTheme={onTheme} className="tb-lang" />
        <LocalePicker className="tb-lang" />

        <div className="crumb" title={title}>
          <span className="crumb-book">{active.name}</span>
          {title && <span className="crumb-title"> · {title}</span>}
        </div>
      </div>

      <div className="reader-body">
        <iframe ref={iframeRef} className="content" title={t("rd.iframeTitle")} />

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
              placeholder={t("find.placeholder")}
              spellCheck={false}
            />
            <button className="tb" onClick={() => doFind(true)} title={t("find.prev")}>
              ↑
            </button>
            <button className="tb" onClick={() => doFind(false)} title={t("find.next")}>
              ↓
            </button>
            <button className="tb" onClick={() => setFindOpen(false)} title={t("find.close")}>
              ✕
            </button>
          </div>
        )}

        {extUrl && (
          <div className="modal-overlay" onClick={() => setExtUrl(null)}>
            <div className="modal ext-modal" onClick={(e) => e.stopPropagation()}>
              <h3>{t("ext.title")}</h3>
              <p className="modal-hint">
                {t("ext.hintPre")} <strong>{active.name}</strong> {t("ext.hintMid")}{" "}
                <strong>{urlHost(extUrl)}</strong>{t("ext.hintPost")}
              </p>
              <div className="ext-url" title={extUrl}>
                {extUrl}
              </div>
              <label className="form-check">
                <input
                  type="checkbox"
                  onChange={(e) => {
                    skipExtWarn.current = e.target.checked;
                  }}
                />
                {t("ext.dontWarn")}
              </label>
              <div className="modal-actions">
                <button
                  className="primary"
                  onClick={() => {
                    openUrl(extUrl).catch(() => {});
                    setExtUrl(null);
                  }}
                >
                  {t("ext.openBrowser")}
                </button>
                <button className="ghost" onClick={() => setExtUrl(null)}>
                  {t("common.cancel")}
                </button>
              </div>
            </div>
          </div>
        )}

        {trOpen && (
          <div className="ftpanel trpanel">
            <div className="ft-head">
              <div className="ft-title">{t("tr.title")}</div>
              <button className="tb" onClick={() => setTrOpen(false)} title={t("common.close")}>
                ✕
              </button>
            </div>

            <div className="tr-langs">
              <span className="tr-label">{t("tr.translateTo")}</span>
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
              <span className="tr-label">{t("tr.articleLang")}</span>
              <select
                className="tr-src"
                value={trSrcOverride}
                onChange={(e) => setTrSrcOverride(e.target.value as Lang | "auto")}
              >
                <option value="auto">
                  {trSrcOverride === "auto" && trSrc
                    ? t("tr.detected", { name: LANG_NAMES[trSrc] })
                    : t("tr.autoDetect")}
                </option>
                {(Object.keys(LANG_NAMES) as Lang[]).map((l) => (
                  <option key={l} value={l}>
                    {LANG_NAMES[l]}
                  </option>
                ))}
              </select>
            </div>

            {!trSrc && <div className="ft-block">{t("tr.noDetect")}</div>}
            {trSrc && trSrc === trTgt && (
              <div className="ft-block">{t("tr.already", { name: LANG_NAMES[trTgt] })}</div>
            )}

            {trDirection && trStatus && !trStatus.legs.every((l) => l.installed) && (
              <div className="ft-block">
                <p>
                  {t("tr.firstTimePre")} <strong>{t("tr.firstTimeStrong")}</strong>{" "}
                  {t("tr.firstTimePost")}
                  {trStatus.legs.length > 1 && <>{t("tr.viaEnglish")}</>}
                </p>
                {trStatus.legs.filter((l) => !l.installed).map((l) => {
                  const dl = trDl[l.leg];
                  return (
                    <div key={l.leg} className="tr-leg">
                      <span className="tr-leg-name">
                        {legName(l.leg)} · {fmtMB(l.bytes)}
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
                            {t("common.cancel")}
                          </button>
                        </>
                      ) : l.downloading ? (
                        <span className="tr-muted">{t("tr.preparing")}</span>
                      ) : null}
                    </div>
                  );
                })}
                {!trStatus.legs.some((l) => l.downloading || trDl[l.leg]) && (
                  <button className="primary" onClick={downloadMissing}>
                    {t(
                      trStatus.legs.filter((l) => !l.installed).length > 1
                        ? "tr.downloadModels"
                        : "tr.downloadModel",
                      {
                        size: fmtMB(
                          trStatus.legs.filter((l) => !l.installed).reduce((s, l) => s + l.bytes, 0),
                        ),
                      },
                    )}
                  </button>
                )}
              </div>
            )}

            {trDirection && trStatus?.legs.every((l) => l.installed) && (
              <div className="ft-block">
                {trPhase === "idle" && (
                  <button className="primary" onClick={() => startTranslateRef.current()}>
                    {t("tr.translatePage")}
                  </button>
                )}
                {trPhase === "loading" && <p>{t("tr.loadingModel")}</p>}
                {trPhase === "translating" && (
                  <>
                    <p>{t("tr.translating", { done: trProgress.done, total: trProgress.total })}</p>
                    <div className="ft-progress">
                      <div
                        style={{
                          width: `${trProgress.total ? (trProgress.done / trProgress.total) * 100 : 0}%`,
                        }}
                      />
                    </div>
                    <button className="ghost" onClick={cancelTranslate}>
                      {t("tr.stop")}
                    </button>
                  </>
                )}
                {trPhase === "done" && (
                  <>
                    <p>{t("tr.doneCached")}</p>
                    <button className="ghost" onClick={toggleOriginal}>
                      {trShowOrig ? t("tr.showTranslation") : t("tr.showOriginal")}
                    </button>
                  </>
                )}
                <label className="tr-auto">
                  <input
                    type="checkbox"
                    checked={trAuto}
                    onChange={(e) => setTrAuto(e.target.checked)}
                  />
                  {t("tr.autoNext")}
                </label>
                <div className="tr-manage">
                  {trStatus.legs.map((l) => (
                    <button
                      key={l.leg}
                      className="tr-remove"
                      title={t("tr.removeModelTitle", { name: legName(l.leg), size: fmtMB(l.bytes) })}
                      onClick={() => removeLeg(l.leg)}
                    >
                      🗑 {legName(l.leg)}
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
              <div className="ft-title">{t("ft.title")}</div>
              <button className="tb" onClick={() => setFtOpen(false)} title={t("common.close")}>
                ✕
              </button>
            </div>
            <div className="ft-query">“{ftQuery}”</div>

            {ftStatus?.state === "none" && (
              <div className="ft-block">
                <p>
                  {t("ft.noIndexPre")} <strong>{t("ft.noIndexStrong")}</strong>{" "}
                  {t("ft.noIndexPost")}
                </p>
                <button className="primary" onClick={() => fulltextBuild(active.id)}>
                  {t("ft.buildNow")}
                </button>
              </div>
            )}

            {ftStatus?.state === "building" && (
              <div className="ft-block">
                <p>{t("ft.indexing", { pct: Math.round((ftStatus.progress ?? 0) * 100) })}</p>
                <div className="ft-progress">
                  <div style={{ width: `${(ftStatus.progress ?? 0) * 100}%` }} />
                </div>
                <button className="ghost" onClick={() => fulltextCancel(active.id)}>
                  {t("common.cancel")}
                </button>
              </div>
            )}

            {ftStatus?.state === "error" && (
              <div className="ft-block">{t("ft.indexFail")}</div>
            )}

            {ftStatus?.state === "ready" && (
              <div className="ft-results">
                {ftBusy && <div className="ft-block">{t("ft.searching")}</div>}
                {!ftBusy && ftResults?.length === 0 && (
                  <div className="ft-block">{t("ft.nothing")}</div>
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
