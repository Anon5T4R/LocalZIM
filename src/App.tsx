import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { closeZim, openZim, startupFile, ZimInfo } from "./lib/backend";
import { saveRecent } from "./lib/recents";
import Library from "./components/Library";
import Reader, { NavTarget } from "./components/Reader";

export default function App() {
  const [books, setBooks] = useState<ZimInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [screen, setScreen] = useState<"library" | "reader">("library");
  const [dark, setDark] = useState(() => localStorage.getItem("localzim.theme") === "dark");
  const [error, setError] = useState<string | null>(null);
  const [nav, setNav] = useState<NavTarget | null>(null);
  // última página visitada de cada livro, para retomar ao alternar
  const lastPaths = useRef(new Map<string, string>());
  const navSeq = useRef(0);

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    localStorage.setItem("localzim.theme", dark ? "dark" : "light");
  }, [dark]);

  const goTo = useCallback((id: string, path: string) => {
    navSeq.current += 1;
    setNav({ id, path, n: navSeq.current });
  }, []);

  const openPath = useCallback(
    async (path: string) => {
      try {
        const info = await openZim(path);
        setBooks((bs) => (bs.some((b) => b.id === info.id) ? bs : [...bs, info]));
        saveRecent(info);
        setActiveId(info.id);
        const dest = lastPaths.current.get(info.id) ?? info.mainPath;
        if (dest) goTo(info.id, dest);
        setScreen("reader");
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [goTo]
  );

  // arquivo passado por associação/linha de comando + segunda instância
  useEffect(() => {
    startupFile().then((f) => {
      if (f) openPath(f);
    });
    const un = listen<string>("open-file", (e) => openPath(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [openPath]);

  const activate = useCallback(
    (id: string) => {
      setScreen("reader");
      if (id === activeId) return;
      setActiveId(id);
      const book = books.find((b) => b.id === id);
      const dest = lastPaths.current.get(id) ?? book?.mainPath;
      if (dest) goTo(id, dest);
    },
    [activeId, books, goTo]
  );

  const closeBook = useCallback(
    async (id: string) => {
      await closeZim(id).catch(() => {});
      lastPaths.current.delete(id);
      setBooks((bs) => {
        const rest = bs.filter((b) => b.id !== id);
        if (id === activeId) {
          setActiveId(null);
          setScreen("library");
        }
        return rest;
      });
    },
    [activeId]
  );

  const onLoaded = useCallback(
    (id: string, path: string) => {
      lastPaths.current.set(id, path);
      // voltar/avançar pode cruzar a fronteira entre livros abertos
      setActiveId((cur) => (cur === id ? cur : id));
    },
    []
  );

  const active = books.find((b) => b.id === activeId) ?? null;

  return (
    <div className="app">
      {screen === "library" && (
        <Library
          books={books}
          error={error}
          onOpenPath={openPath}
          onActivate={activate}
          onCloseBook={closeBook}
        />
      )}
      {active && (
        <div className={screen === "reader" ? "reader-wrap" : "reader-wrap hidden"}>
          <Reader
            active={active}
            nav={nav}
            dark={dark}
            onToggleDark={() => setDark((d) => !d)}
            onLibrary={() => setScreen("library")}
            onLoaded={onLoaded}
          />
        </div>
      )}
    </div>
  );
}
