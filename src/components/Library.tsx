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
import { t, localeTag } from "../lib/i18n";
import LocalePicker from "./LocalePicker";

interface Props {
  books: ZimInfo[];
  error: string | null;
  onOpenPath: (path: string) => void;
  onActivate: (id: string) => void;
  onCloseBook: (id: string) => void;
}

function hostOf(url: string): string {
  const u = url.trim();
  for (const candidate of [u, `https://${u}`]) {
    try {
      return new URL(candidate).hostname;
    } catch {
      /* tenta a próxima forma */
    }
  }
  return "";
}

function metaLine(language: string, size: number, articles: number | null): string {
  const parts: string[] = [];
  if (language) parts.push(language);
  if (size > 0) parts.push(formatBytes(size));
  if (articles != null) parts.push(t("lib.articles", { n: articles.toLocaleString(localeTag()) }));
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
      title: t("dlg.openZim"),
      multiple: false,
      filters: [{ name: t("dlg.zimFiles"), extensions: ["zim"] }],
    });
    if (typeof sel === "string") onOpenPath(sel);
  };

  const pickSourceDir = async () => {
    const sel = await openDialog({ title: t("dlg.sourceDir"), directory: true });
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
      title: t("dlg.saveZim"),
      defaultPath: `${(cTitle || "biblioteca").replace(/[\\/:*?"<>|]/g, "-")}.zim`,
      filters: [{ name: t("dlg.zimFile"), extensions: ["zim"] }],
    });
    if (typeof sel === "string") setCOutput(sel);
  };

  const startCreate = async () => {
    if (!cSource) {
      setCState({ state: "error", progress: 0, error: t("err.noSource") });
      return;
    }
    if (!cOutput) {
      setCState({
        state: "error",
        progress: 0,
        error: t("err.noOutput"),
      });
      return;
    }
    setCState({ state: "building", progress: 0 });
    try {
      await createZim({
        source: cSource,
        output: cOutput,
        title: cTitle.trim() || t("lib.defaultLibrary"),
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
  const [sDepth, setSDepth] = useState("100");
  const [sMaxPages, setSMaxPages] = useState("2000");
  const [sSamePath, setSSamePath] = useState(true);

  const pickSiteOutput = async () => {
    const sel = await saveDialog({
      title: t("dlg.saveZim"),
      defaultPath: `${(sTitle || "site").replace(/[\\/:*?"<>|]/g, "-")}.zim`,
      filters: [{ name: t("dlg.zimFile"), extensions: ["zim"] }],
    });
    if (typeof sel === "string") setSOutput(sel);
  };

  const startSite = async () => {
    // validação com feedback — botão desabilitado sem aviso é clique morto
    if (!sUrl.trim()) {
      setCState({ state: "error", progress: 0, error: t("err.noUrl") });
      return;
    }
    if (!sOutput) {
      setCState({
        state: "error",
        progress: 0,
        error: t("err.noOutput"),
      });
      return;
    }
    setCState({ state: "building", progress: 0, phase: "crawl", pages: 0 });
    try {
      await createZimFromSite({
        url: sUrl.trim(),
        output: sOutput,
        title: sTitle.trim() || hostOf(sUrl) || t("lib.defaultSite"),
        description: sDesc,
        language: sLang,
        maxDepth: Math.max(0, parseInt(sDepth, 10) || 100),
        maxPages: Math.max(1, parseInt(sMaxPages, 10) || 2000),
        samePath: sSamePath,
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
          <p className="lib-sub">{t("lib.sub")}</p>
        </div>
        <div className="lib-actions">
          <button className="primary" onClick={pickFile}>
            {t("lib.openBtn")}
          </button>
          <button
            className="secondary"
            onClick={() => {
              setCState(null);
              setCreateOpen(true);
            }}
            title={t("lib.createFolderTitle")}
          >
            {t("lib.createFolder")}
          </button>
          <button
            className="secondary"
            onClick={() => {
              setCState(null);
              setSiteOpen(true);
            }}
            title={t("lib.createSiteTitle")}
          >
            {t("lib.createSite")}
          </button>
          <LocalePicker />
        </div>
      </header>

      {error && <div className="error-banner">{error}</div>}

      {books.length > 0 && (
        <section>
          <h2>{t("lib.open")}</h2>
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
                  title={t("lib.closeFile")}
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
          <h2>{t("lib.recent")}</h2>
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
                  title={t("lib.removeRecent")}
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
            {t("lib.emptyPre")} <strong>.zim</strong> {t("lib.emptyMid")}{" "}
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
            {t("lib.openBtn")}
          </button>
        </div>
      )}

      {siteOpen && (
        <div className="modal-overlay" onClick={() => !building && setSiteOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>{t("site.title")}</h3>
            <p className="modal-hint">
              {t("site.hintPre")}{" "}
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
              <label>{t("site.url")}</label>
              <input
                value={sUrl}
                onChange={(e) => {
                  setSUrl(e.target.value);
                  if (!sTitle) setSTitle(hostOf(e.target.value));
                }}
                disabled={building}
                placeholder={t("site.urlPlaceholder")}
                spellCheck={false}
              />
            </div>
            <div className="form-row">
              <label>{t("form.saveAs")}</label>
              <div className="form-pick">
                <input value={sOutput} readOnly placeholder={t("form.outputPlaceholder")} />
                <button onClick={pickSiteOutput} disabled={building}>
                  {t("form.choose")}
                </button>
              </div>
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>{t("form.title")}</label>
                <input value={sTitle} onChange={(e) => setSTitle(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>{t("form.language")}</label>
                <input value={sLang} onChange={(e) => setSLang(e.target.value)} disabled={building} />
              </div>
            </div>
            <div className="form-row">
              <label>{t("form.description")}</label>
              <input value={sDesc} onChange={(e) => setSDesc(e.target.value)} disabled={building} />
            </div>
            <label className="form-check">
              <input
                type="checkbox"
                checked={sSamePath}
                onChange={(e) => setSSamePath(e.target.checked)}
                disabled={building}
              />
              {t("site.samePathPre")} <code>/book/</code>
              {t("site.samePathPost")}
            </label>
            <div className="form-grid">
              <div className="form-row">
                <label>{t("site.depth")}</label>
                <input value={sDepth} onChange={(e) => setSDepth(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>{t("site.maxPages")}</label>
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
                    ? t("site.crawling", { pages: cState?.pages ?? 0, queued: cState?.queued ?? 0 })
                    : t("folder.packing", { pct: Math.round((cState?.progress ?? 0) * 100) })}
                </p>
                <div className="ft-progress">
                  <div style={{ width: `${(cState?.progress ?? 0) * 100}%` }} />
                </div>
              </div>
            )}
            {cState?.state === "error" && <div className="error-banner">{cState.error}</div>}
            {cState?.state === "done" && cState.result && (
              <div className="ok-banner">
                {t("site.doneBanner", {
                  articles: cState.result.articles,
                  size: formatBytes(cState.result.size),
                })}
              </div>
            )}

            <div className="modal-actions">
              {!building && (
                <button className="primary" onClick={startSite}>
                  {t("site.download")}
                </button>
              )}
              {building && (
                <button className="ghost" onClick={() => cancelCreateZim()}>
                  {t("common.cancel")}
                </button>
              )}
              <button disabled={building} onClick={() => setSiteOpen(false)}>
                {t("common.close")}
              </button>
            </div>
          </div>
        </div>
      )}

      {createOpen && (
        <div className="modal-overlay" onClick={() => !building && setCreateOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>{t("folder.title")}</h3>
            <p className="modal-hint">
              {t("folder.hintPre")} <code>.zim</code>. {t("folder.hintMid")}{" "}
              <a
                href="https://github.com/openzim/zimit"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl("https://github.com/openzim/zimit").catch(() => {});
                }}
              >
                zimit
              </a>{" "}
              {t("folder.hintPost")} <code>wget --mirror</code> {t("folder.hintEnd")}
            </p>

            <div className="form-row">
              <label>{t("folder.source")}</label>
              <div className="form-pick">
                <input value={cSource} readOnly placeholder={t("folder.sourcePlaceholder")} />
                <button onClick={pickSourceDir} disabled={building}>
                  {t("form.choose")}
                </button>
              </div>
            </div>
            <div className="form-row">
              <label>{t("form.saveAs")}</label>
              <div className="form-pick">
                <input value={cOutput} readOnly placeholder={t("form.outputPlaceholder")} />
                <button onClick={pickOutput} disabled={building}>
                  {t("form.choose")}
                </button>
              </div>
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>{t("form.title")}</label>
                <input value={cTitle} onChange={(e) => setCTitle(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>{t("form.language")}</label>
                <input value={cLang} onChange={(e) => setCLang(e.target.value)} disabled={building} placeholder="por" />
              </div>
            </div>
            <div className="form-row">
              <label>{t("form.description")}</label>
              <input value={cDesc} onChange={(e) => setCDesc(e.target.value)} disabled={building} />
            </div>
            <div className="form-grid">
              <div className="form-row">
                <label>{t("folder.creator")}</label>
                <input value={cCreator} onChange={(e) => setCCreator(e.target.value)} disabled={building} />
              </div>
              <div className="form-row">
                <label>{t("folder.mainPage")}</label>
                <input
                  value={cMain}
                  onChange={(e) => setCMain(e.target.value)}
                  disabled={building}
                  placeholder={t("folder.mainPlaceholder")}
                />
              </div>
            </div>

            {building && (
              <div className="ft-block" style={{ padding: "10px 0 0" }}>
                <p style={{ margin: "0 0 6px" }}>
                  {t("folder.packing", { pct: Math.round((cState?.progress ?? 0) * 100) })}
                </p>
                <div className="ft-progress">
                  <div style={{ width: `${(cState?.progress ?? 0) * 100}%` }} />
                </div>
              </div>
            )}
            {cState?.state === "error" && <div className="error-banner">{cState.error}</div>}
            {cState?.state === "done" && cState.result && (
              <div className="ok-banner">
                {t("folder.doneBanner", {
                  articles: cState.result.articles,
                  size: formatBytes(cState.result.size),
                })}
              </div>
            )}

            <div className="modal-actions">
              {!building && (
                <button className="primary" onClick={startCreate}>
                  {t("folder.create")}
                </button>
              )}
              {building && (
                <button className="ghost" onClick={() => cancelCreateZim()}>
                  {t("common.cancel")}
                </button>
              )}
              <button disabled={building} onClick={() => setCreateOpen(false)}>
                {t("common.close")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
