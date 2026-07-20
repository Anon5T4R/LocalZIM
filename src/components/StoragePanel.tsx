import { useCallback, useEffect, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  storageClearIncomplete,
  storageClearTranslateCache,
  storageDeleteIndex,
  storageInfo,
  type IndexEntry,
  type KnownZim,
  type StorageInfo,
} from "../lib/backend";
import { formatBytes } from "../lib/paths";
import { t } from "../lib/i18n";
import { loadRecents } from "../lib/recents";
import type { ZimInfo } from "../lib/backend";

interface Props {
  /** Livros abertos agora — entram na lista de "conhecidos" junto dos recentes. */
  books: ZimInfo[];
  onClose: () => void;
}

/** O que está prestes a ser apagado (null = nada perguntado). */
type Pending =
  | { kind: "incomplete" }
  | { kind: "cache" }
  | { kind: "index"; uuid: string; name: string };

/**
 * Painel "Dados e armazenamento". A decisão de desenho que manda aqui: os
 * índices são LISTADOS um a um, com o nome do livro e o tamanho, em vez de um
 * botão "limpar órfãos". O app não consegue provar que um .zim saiu do disco —
 * ele só sabe o que está nos recentes (24 entradas) — então quem decide é o
 * usuário, que reconhece o nome. Ver o cabeçalho de `storage.rs`.
 */
export default function StoragePanel({ books, onClose }: Props) {
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");

  /** Recentes + abertos = tudo o que o app sabe que o usuário tem. */
  const known = useCallback((): KnownZim[] => {
    const list: KnownZim[] = loadRecents().map((r) => ({ path: r.path, size: r.size }));
    for (const b of books) list.push({ path: b.path, size: b.size });
    return list;
  }, [books]);

  const refresh = useCallback(async () => {
    try {
      setInfo(await storageInfo(known()));
    } catch (e) {
      setMsg(t("storage.failed", { e: String(e) }));
    }
  }, [known]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function run(p: Pending) {
    setPending(null);
    setBusy(true);
    try {
      const freed =
        p.kind === "incomplete"
          ? await storageClearIncomplete(known())
          : p.kind === "cache"
            ? await storageClearTranslateCache()
            : await storageDeleteIndex(p.uuid);
      setMsg(
        freed.files === 0
          ? t("storage.nothing")
          : t("storage.freed", { size: formatBytes(freed.bytes), n: freed.files }),
      );
      await refresh();
    } catch (e) {
      setMsg(t("storage.failed", { e: String(e) }));
    } finally {
      setBusy(false);
    }
  }

  /** Etiqueta de estado de uma linha, e a explicação dela. */
  function rowTag(e: IndexEntry): string {
    if (!e.ready) return t("storage.incompleteTag");
    if (!e.labeled) return t("storage.noLabel");
    return e.known ? t("storage.have") : t("storage.notFound");
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal storage-modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t("storage.title")}</h2>

        {info && (
          <>
            <div className="storage-row">
              <div className="storage-label">
                <span>{t("storage.path")}</span>
                <code className="storage-dir" title={info.dir}>
                  {info.dir}
                </code>
              </div>
              <button className="secondary" onClick={() => void openPath(info.dir).catch(() => {})}>
                {t("storage.openFolder")}
              </button>
            </div>

            <div className="storage-row">
              <div className="storage-label">
                <span>
                  {t("storage.fulltext")} — <strong>{formatBytes(info.fulltextBytes)}</strong>
                </span>
                <small>
                  {t("storage.fulltextCounts", {
                    n: info.indexes.length,
                    ready: info.readyCount,
                    known: info.knownCount,
                  })}
                </small>
                <small>{t("storage.fulltextHint")}</small>
              </div>
            </div>

            <div className="storage-row">
              <div className="storage-label">
                <span>{t("storage.incomplete")}</span>
                <small>
                  {t("storage.incompleteCounts", {
                    n: info.incompleteCount,
                    size: formatBytes(info.incompleteBytes),
                  })}
                </small>
                <small>{t("storage.incompleteHint")}</small>
              </div>
              <button
                className="secondary"
                disabled={busy || info.incompleteCount === 0}
                onClick={() => setPending({ kind: "incomplete" })}
              >
                {t("storage.clear")}
              </button>
            </div>

            <div className="storage-list">
              <strong>{t("storage.list")}</strong>
              <small>{t("storage.listHint")}</small>
              {info.indexes.length === 0 && <p className="storage-empty">{t("storage.empty")}</p>}
              {info.indexes.map((e) => (
                <div className="storage-row" key={e.uuid}>
                  <div className="storage-label">
                    {/* Sem etiqueta não há nome: mostrar o uuid é honesto — o
                        usuário vê que o app não sabe, em vez de um nome errado. */}
                    <span>
                      {e.name || e.uuid} — <strong>{formatBytes(e.bytes)}</strong>
                    </span>
                    <small className={e.known ? "tag-ok" : "tag-warn"}>{rowTag(e)}</small>
                    {e.fileName && <small className="storage-dir">{e.fileName}</small>}
                  </div>
                  <button
                    className="secondary"
                    disabled={busy}
                    onClick={() =>
                      setPending({ kind: "index", uuid: e.uuid, name: e.name || e.uuid })
                    }
                  >
                    {t("storage.delete")}
                  </button>
                </div>
              ))}
            </div>

            {/* Medido e nunca apagado daqui: os modelos têm a tela deles. */}
            <div className="storage-row">
              <div className="storage-label">
                <span>
                  {t("storage.models")} — <strong>{formatBytes(info.modelsBytes)}</strong>
                </span>
                <small>{t("storage.modelsCounts", { n: info.modelsFiles })}</small>
                <small>{t("storage.modelsHint")}</small>
              </div>
            </div>

            <div className="storage-row">
              <div className="storage-label">
                <span>
                  {t("storage.cache")} — <strong>{formatBytes(info.cacheBytes)}</strong>
                </span>
                <small>{t("storage.cacheCounts", { n: info.cacheFiles })}</small>
                <small>{t("storage.cacheHint")}</small>
              </div>
              <button
                className="secondary"
                disabled={busy || info.cacheFiles === 0}
                onClick={() => setPending({ kind: "cache" })}
              >
                {t("storage.clear")}
              </button>
            </div>
          </>
        )}

        {pending && (
          <div className="storage-confirm">
            <strong>{t("storage.confirmTitle")}</strong>
            <p>
              {pending.kind === "incomplete"
                ? t("storage.confirmIncomplete")
                : pending.kind === "cache"
                  ? t("storage.confirmCache")
                  : t("storage.confirmIndex", { name: pending.name })}
            </p>
            <div className="modal-actions">
              <button onClick={() => setPending(null)}>{t("common.cancel")}</button>
              <button className="danger" onClick={() => void run(pending)}>
                {t("storage.confirmYes")}
              </button>
            </div>
          </div>
        )}

        {msg && <div className="ok-banner">{msg}</div>}

        <div className="modal-actions">
          <button onClick={onClose}>{t("common.close")}</button>
        </div>
      </div>
    </div>
  );
}
