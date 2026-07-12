import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Suggestion, ZimInfo, zimRandom, zimSuggest } from "../lib/backend";
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

  const [query, setQuery] = useState("");
  const [sugs, setSugs] = useState<Suggestion[]>([]);
  const [showSugs, setShowSugs] = useState(false);
  const [sel, setSel] = useState(0);

  const postToFrame = (msg: unknown) => {
    iframeRef.current?.contentWindow?.postMessage(msg, "*");
  };

  // navegação imperativa vinda do App (abrir livro, trocar de livro)
  useEffect(() => {
    if (!nav) return;
    const el = iframeRef.current;
    if (el) el.src = zimUrl(nav.id, nav.path);
  }, [nav]);

  // zoom lembrado por livro
  useEffect(() => {
    const z = zoomMap.current.get(active.id) ?? 100;
    zoomRef.current = z;
    setZoom(z);
  }, [active.id]);

  // tema do artigo acompanha o tema do app
  useEffect(() => {
    postToFrame({ type: "zim:dark", on: dark });
  }, [dark]);

  // mensagens da ponte injetada nas páginas
  useEffect(() => {
    const onMsg = (ev: MessageEvent) => {
      const d = (ev.data ?? {}) as { type?: string; href?: string; title?: string; url?: string };
      if (d.type === "zim:loaded") {
        const parsed = pathFromHref(String(d.href ?? ""));
        const t = String(d.title ?? "");
        setTitle(t);
        if (parsed) onLoadedRef.current(parsed.id, parsed.path, t);
        // reaplica preferências na página recém-carregada
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
      }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
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

  const goRandom = async () => {
    const p = await zimRandom(active.id).catch(() => null);
    if (p) navigate(p);
  };

  const onSearchKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!showSugs || sugs.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => (s + 1) % sugs.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => (s - 1 + sugs.length) % sugs.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      navigate(sugs[Math.min(sel, sugs.length - 1)].path);
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
        <button className="tb" onClick={() => history.back()} title="Voltar">
          ←
        </button>
        <button className="tb" onClick={() => history.forward()} title="Avançar">
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
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKey}
            onFocus={() => sugs.length > 0 && setShowSugs(true)}
            onBlur={() => setTimeout(() => setShowSugs(false), 150)}
            placeholder={`Buscar em ${active.name}…`}
            spellCheck={false}
          />
          {showSugs && sugs.length > 0 && (
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
            </ul>
          )}
        </div>

        <div className="zoom">
          <button className="tb" onClick={() => applyZoom(zoom - 10)} title="Diminuir zoom">
            −
          </button>
          <span className="zoom-label">{zoom}%</span>
          <button className="tb" onClick={() => applyZoom(zoom + 10)} title="Aumentar zoom">
            +
          </button>
        </div>

        <button className="tb" onClick={onToggleDark} title="Alternar tema claro/escuro">
          {dark ? "☀️" : "🌙"}
        </button>

        <div className="crumb" title={title}>
          <span className="crumb-book">{active.name}</span>
          {title && <span className="crumb-title"> · {title}</span>}
        </div>
      </div>
      <iframe ref={iframeRef} className="content" title="Conteúdo do arquivo ZIM" />
    </div>
  );
}
