//! Painel "Dados e armazenamento": mede o que o LocalZIM ocupa em disco e
//! oferece limpezas que dizem exatamente o que apagam.
//!
//! O peso daqui não é o app: é o **índice full-text**. Indexar uma Wikipédia
//! inteira leva muitos minutos e produz centenas de MB em
//! `app_data/fulltext/<uuid do ZIM>/`. E esse índice **nunca era apagado**:
//! quando o arquivo .zim saía do disco, o índice dele ficava pra sempre, sem
//! nada na UI dando um pio — é a categoria de lixo que este painel resolve.
//!
//! ## Como se sabe a quem um índice pertence (e por que NÃO por caminho)
//!
//! A pasta é nomeada pelo UUID do ZIM, que é intrínseco ao arquivo. Do hex não
//! dá pra voltar ao nome do livro, então cada índice ganha uma etiqueta
//! `localzim-source.json` com nome, nome de ARQUIVO e tamanho do ZIM que o
//! gerou.
//!
//! O reconhecimento compara **nome de arquivo + tamanho** contra os ZIMs que o
//! usuário conhece (os recentes). Comparar o CAMINHO seria a versão óbvia e é
//! destrutiva: o caminho foi gravado quando o ZIM morava em outro lugar, e
//! quem move a biblioteca de pasta (ou troca a letra da unidade, ou reinstala
//! o app num perfil novo) veria TODOS os índices virarem órfãos — e perderia
//! horas de indexação numa limpeza que relataria sucesso. Nome de arquivo e
//! tamanho de um ZIM não mudam quando ele muda de lugar; é isso que se compara.
//!
//! ## Baldes, por quanto dá pra provar
//!
//! - **incompleto** — sem o `localzim-done.json`: indexação interrompida. O
//!   app já ignora esse índice e reindexa do zero, então apagá-lo é risco zero.
//! - **reconhecido** — o ZIM está nos seus recentes. Nenhuma limpeza encosta.
//! - **não reconhecido** — não achamos o ZIM. Pode ser arquivo que saiu do
//!   disco… ou um ZIM que você simplesmente não abre há tempo (os recentes
//!   guardam 24). Por isso o painel LISTA cada um **pelo nome do livro** e
//!   deixa você decidir um a um, em vez de apagar em bloco no escuro. Quem
//!   sabe se o arquivo ainda existe é você, não o app.

use std::path::{Path, PathBuf};

use tauri::Manager;

/// Etiqueta que amarra um índice ao ZIM que o gerou.
pub const LABEL: &str = "localzim-source.json";
/// Marca de índice COMPLETO, escrita pelo `search::build` no fim (mantida em
/// sincronia com o `search::DONE_FILE` — o teste `marca_de_pronto_bate` cobra).
pub const DONE: &str = "localzim-done.json";

#[derive(serde::Serialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Freed {
    pub files: u64,
    pub bytes: u64,
}

/// Etiqueta de um índice. `file_name` + `size` são a identidade que sobrevive a
/// mudança de pasta; `path` fica só pra mostrar de onde ele veio.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    /// Nome do livro (o metadado do ZIM) — é isto que a UI mostra.
    pub name: String,
    pub file_name: String,
    pub size: u64,
    pub path: String,
}

/// Um ZIM que o usuário conhece (vem dos recentes do front).
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KnownZim {
    pub path: String,
    pub size: u64,
}

pub fn tree_stats(dir: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let (b, f) = tree_stats(&path);
                bytes += b;
                files += f;
            } else if let Ok(meta) = entry.metadata() {
                bytes += meta.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn remove_tree(dir: &Path) -> Freed {
    let (bytes, files) = tree_stats(dir);
    if std::fs::remove_dir_all(dir).is_ok() {
        Freed { files, bytes }
    } else {
        Freed::default()
    }
}

fn remove_trees(dirs: &[PathBuf]) -> Freed {
    let mut total = Freed::default();
    for d in dirs {
        let f = remove_tree(d);
        total.files += f.files;
        total.bytes += f.bytes;
    }
    total
}

pub fn read_label(dir: &Path) -> Option<Label> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(LABEL)).ok()?).ok()
}

/// Grava a etiqueta. Best-effort: falhar aqui não pode derrubar a indexação —
/// o índice só nasce "sem etiqueta", que é o balde conservador.
pub fn write_label(dir: &Path, label: &Label) {
    if let Ok(json) = serde_json::to_string(label) {
        let _ = std::fs::write(dir.join(LABEL), json);
    }
}

/// Nome de arquivo em minúsculas (a comparação é insensível a caixa porque o
/// Windows é).
fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// O ZIM desta etiqueta está entre os que o usuário conhece?
///
/// Compara nome de arquivo + tamanho, NUNCA o caminho — ver o cabeçalho do
/// módulo. Etiqueta sem nome de arquivo (não deveria acontecer) nunca casa: na
/// dúvida o índice fica, e ficar custa disco, enquanto sumir custa horas.
pub fn is_known(label: &Label, known: &[KnownZim]) -> bool {
    let mine = if label.file_name.is_empty() {
        base_name(&label.path)
    } else {
        label.file_name.to_lowercase()
    };
    if mine.is_empty() {
        return false;
    }
    known
        .iter()
        .any(|k| base_name(&k.path) == mine && (label.size == 0 || k.size == 0 || k.size == label.size))
}

/// Uma pasta de índice, já classificada, do jeito que o painel a mostra.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    /// Nome da pasta = uuid_hex do ZIM. É a chave que a UI devolve pra apagar.
    pub uuid: String,
    /// Nome do livro, se houver etiqueta; senão vazio (a UI mostra o uuid).
    pub name: String,
    pub file_name: String,
    pub bytes: u64,
    /// Indexação terminou? `false` = interrompida, o app reindexaria de qualquer jeito.
    pub ready: bool,
    /// O ZIM está nos recentes.
    pub known: bool,
    /// Tem etiqueta (índice criado por esta versão em diante).
    pub labeled: bool,
}

/// Varre `fulltext/` e classifica cada índice.
pub fn scan(root: &Path, known: &[KnownZim]) -> Vec<IndexEntry> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let label = read_label(&dir);
            out.push(IndexEntry {
                uuid: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                name: label.as_ref().map(|l| l.name.clone()).unwrap_or_default(),
                file_name: label.as_ref().map(|l| l.file_name.clone()).unwrap_or_default(),
                bytes: tree_stats(&dir).0,
                ready: dir.join(DONE).exists(),
                known: label.as_ref().map(|l| is_known(l, known)).unwrap_or(false),
                labeled: label.is_some(),
            });
        }
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes)); // o que pesa primeiro
    out
}

/// Índices que a indexação não terminou de escrever. Lixo provado: sem o
/// `localzim-done.json` o `search::is_ready` devolve None e o app reindexa do
/// zero, então esses bytes não servem pra nada hoje.
pub fn incomplete(entries: &[IndexEntry]) -> Vec<String> {
    entries.iter().filter(|e| !e.ready).map(|e| e.uuid.clone()).collect()
}

/// Índices completos cujo ZIM não está nos recentes. NÃO é sinônimo de órfão —
/// ver o cabeçalho; por isso a UI lista um a um.
pub fn unrecognized(entries: &[IndexEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.ready && e.labeled && !e.known)
        .map(|e| e.uuid.clone())
        .collect()
}

/// Só nomes de pasta simples (hex do uuid) podem virar caminho a apagar. Sem
/// isto, um `uuid` vindo da UI com `..` apagaria pasta de fora do cache.
fn safe_child(root: &Path, uuid: &str) -> Option<PathBuf> {
    if uuid.is_empty() || !uuid.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(root.join(uuid))
}

// ---------------------------------------------------------------------------
// comandos
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    dir: String,
    fulltext_bytes: u64,
    fulltext_files: u64,
    indexes: Vec<IndexEntry>,
    ready_count: u64,
    known_count: u64,
    incomplete_bytes: u64,
    incomplete_count: u64,
    unrecognized_bytes: u64,
    unrecognized_count: u64,
    unlabeled_count: u64,
    models_bytes: u64,
    models_files: u64,
    cache_bytes: u64,
    cache_files: u64,
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn fulltext_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("fulltext"))
}

fn sum_of(root: &Path, uuids: &[String]) -> u64 {
    uuids.iter().filter_map(|u| safe_child(root, u)).map(|p| tree_stats(&p).0).sum()
}

#[tauri::command(async)]
pub fn storage_info(app: tauri::AppHandle, known: Vec<KnownZim>) -> Result<StorageInfo, String> {
    let dir = data_dir(&app)?;
    let root = dir.join("fulltext");
    let (fulltext_bytes, fulltext_files) = tree_stats(&root);
    let indexes = scan(&root, &known);

    let inc = incomplete(&indexes);
    let unk = unrecognized(&indexes);
    let (models_bytes, models_files) = tree_stats(&dir.join("translate").join("models"));
    let (cache_bytes, cache_files) = tree_stats(&dir.join("translate").join("cache"));

    Ok(StorageInfo {
        dir: dir.to_string_lossy().into_owned(),
        fulltext_bytes,
        fulltext_files,
        ready_count: indexes.iter().filter(|e| e.ready).count() as u64,
        known_count: indexes.iter().filter(|e| e.known).count() as u64,
        incomplete_bytes: sum_of(&root, &inc),
        incomplete_count: inc.len() as u64,
        unrecognized_bytes: sum_of(&root, &unk),
        unrecognized_count: unk.len() as u64,
        unlabeled_count: indexes.iter().filter(|e| !e.labeled).count() as u64,
        indexes,
        models_bytes,
        models_files,
        cache_bytes,
        cache_files,
    })
}

/// Índices de indexação interrompida. Risco zero.
#[tauri::command(async)]
pub fn storage_clear_incomplete(
    app: tauri::AppHandle,
    known: Vec<KnownZim>,
) -> Result<Freed, String> {
    let root = fulltext_root(&app)?;
    let alvo = incomplete(&scan(&root, &known));
    Ok(remove_trees(&alvo.iter().filter_map(|u| safe_child(&root, u)).collect::<Vec<_>>()))
}

/// Apaga UM índice, pelo uuid que a UI listou com nome e tamanho. É este o
/// caminho principal: quem sabe se o ZIM ainda existe é o usuário.
#[tauri::command(async)]
pub fn storage_delete_index(app: tauri::AppHandle, uuid: String) -> Result<Freed, String> {
    let root = fulltext_root(&app)?;
    let dir = safe_child(&root, &uuid).ok_or_else(|| format!("uuid inválido: {uuid}"))?;
    if !dir.is_dir() {
        return Ok(Freed::default());
    }
    Ok(remove_tree(&dir))
}

/// Cache de tradução (frases já traduzidas). Regenerável: o modelo continua lá.
#[tauri::command(async)]
pub fn storage_clear_translate_cache(app: tauri::AppHandle) -> Result<Freed, String> {
    let dir = data_dir(&app)?.join("translate").join("cache");
    let freed = remove_tree(&dir);
    let _ = std::fs::create_dir_all(&dir);
    Ok(freed)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn tmp(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("localzim-storage-{tag}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn label(name: &str, file_name: &str, size: u64, path: &str) -> Label {
        Label {
            name: name.into(),
            file_name: file_name.into(),
            size,
            path: path.into(),
        }
    }

    /// Cria uma pasta de índice: `ready` escreve a marca de completo.
    fn index(root: &Path, uuid: &str, bytes: usize, ready: bool, l: Option<&Label>) -> PathBuf {
        let dir = root.join(uuid);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), vec![b'x'; bytes]).unwrap();
        if ready {
            std::fs::write(dir.join(DONE), r#"{"docs":10}"#).unwrap();
        }
        if let Some(l) = l {
            write_label(&dir, l);
        }
        dir
    }

    fn known(path: &str, size: u64) -> KnownZim {
        KnownZim { path: path.into(), size }
    }

    /// A marca de completo tem que ser a MESMA que o search.rs escreve; se
    /// alguém renomear lá, o painel passaria a chamar todo índice pronto de
    /// "interrompido" e ofereceria apagar tudo.
    #[test]
    fn marca_de_pronto_bate_com_a_do_indexador() {
        assert_eq!(DONE, crate::search::DONE_FILE);
    }

    /// O caso que motivou a comparação por nome: a biblioteca mudou de pasta
    /// (e de letra de unidade). Comparar caminho tornaria todos os índices
    /// "órfãos" e a limpeza jogaria fora horas de indexação.
    #[test]
    fn biblioteca_movida_continua_reconhecida() {
        let l = label("Wikipédia (pt)", "wikipedia_pt_all.zim", 90_000_000_000, "E:/zim/wikipedia_pt_all.zim");
        // Mesmo arquivo, outro lugar: reconhecido.
        assert!(is_known(&l, &[known("D:/Biblioteca/ZIM/wikipedia_pt_all.zim", 90_000_000_000)]));
        // Outro arquivo, mesmo tamanho: não é o mesmo livro.
        assert!(!is_known(&l, &[known("D:/ZIM/wikcionario_pt.zim", 90_000_000_000)]));
        // Mesmo nome, tamanho diferente: é outra edição do dump, índice não serve.
        assert!(!is_known(&l, &[known("D:/ZIM/wikipedia_pt_all.zim", 12_345)]));
        // Caixa diferente (Windows não distingue).
        assert!(is_known(&l, &[known("D:/ZIM/WIKIPEDIA_PT_ALL.ZIM", 90_000_000_000)]));
        // Ninguém conhecido.
        assert!(!is_known(&l, &[]));
    }

    /// Etiqueta sem nome de arquivo cai no caminho como último recurso — e
    /// etiqueta sem nada nunca "casa" com tudo (curinga seria catastrófico ao
    /// contrário: aqui casar demais só preserva, mas não pode preservar por
    /// engano um índice que a UI então nunca ofereceria apagar).
    #[test]
    fn etiqueta_sem_nome_nao_vira_curinga() {
        let so_caminho = label("X", "", 0, "D:/ZIM/a.zim");
        assert!(is_known(&so_caminho, &[known("E:/outro/a.zim", 0)]));
        assert!(!is_known(&Label::default(), &[known("D:/ZIM/a.zim", 1)]));
    }

    #[test]
    fn interrompido_e_alvo_pronto_e_reconhecido_nao() {
        let root = tmp("baldes");
        let l = label("Livro", "livro.zim", 500, "D:/z/livro.zim");
        index(&root, "aaaa", 100, true, Some(&l)); // pronto e reconhecido
        index(&root, "bbbb", 200, false, Some(&l)); // interrompido
        index(&root, "cccc", 300, true, Some(&label("Sumido", "sumido.zim", 9, "D:/z/sumido.zim")));

        let k = vec![known("D:/z/livro.zim", 500)];
        let e = scan(&root, &k);
        assert_eq!(e.len(), 3);

        assert_eq!(incomplete(&e), vec!["bbbb".to_string()]);
        assert_eq!(unrecognized(&e), vec!["cccc".to_string()]);

        // O índice bom não aparece em NENHUMA lista de limpeza.
        assert!(!incomplete(&e).contains(&"aaaa".to_string()));
        assert!(!unrecognized(&e).contains(&"aaaa".to_string()));

        // E depois de rodar a limpeza de risco zero, ele continua inteiro.
        remove_trees(&incomplete(&e).iter().filter_map(|u| safe_child(&root, u)).collect::<Vec<_>>());
        assert!(root.join("aaaa").join(DONE).exists(), "índice pronto foi apagado");
        assert!(root.join("cccc").exists(), "limpeza de interrompidos levou um pronto");
        assert!(!root.join("bbbb").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Índice criado por versão anterior não tem etiqueta. Ele NUNCA pode entrar
    /// em "não reconhecido": sem etiqueta não dá pra saber de quem é, e a
    /// primeira execução depois de atualizar ofereceria apagar o cache inteiro.
    #[test]
    fn indice_de_versao_antiga_nao_entra_em_nao_reconhecido() {
        let root = tmp("sem-etiqueta");
        index(&root, "velho", 400, true, None);
        let e = scan(&root, &[]);
        assert!(unrecognized(&e).is_empty(), "índice sem etiqueta virou alvo");
        assert!(incomplete(&e).is_empty());
        assert_eq!(e[0].labeled, false);
        assert_eq!(e[0].name, "", "sem etiqueta não há nome pra mostrar");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sem etiqueta E interrompido: o balde "interrompido" vale, porque não
    /// depende de saber de quem é — o índice não serve pra ninguém.
    #[test]
    fn sem_etiqueta_mas_interrompido_ainda_e_lixo() {
        let root = tmp("sem-etiqueta-interrompido");
        index(&root, "meio", 400, false, None);
        assert_eq!(incomplete(&scan(&root, &[])), vec!["meio".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A UI manda o uuid de volta pra apagar. Nome de pasta com travessia teria
    /// que apagar coisa de fora do cache.
    #[test]
    fn uuid_torto_nao_vira_caminho() {
        let root = tmp("travessia");
        assert!(safe_child(&root, "..").is_none());
        assert!(safe_child(&root, "../../Windows").is_none());
        assert!(safe_child(&root, "a/b").is_none());
        assert!(safe_child(&root, "").is_none());
        assert!(safe_child(&root, "deadbeef0123").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apagar_um_indice_leva_so_ele() {
        let root = tmp("apagar-um");
        index(&root, "aaa", 100, true, None);
        index(&root, "bbb", 200, true, None);
        let freed = remove_tree(&safe_child(&root, "bbb").unwrap());
        assert_eq!(freed.files, 2); // meta.json + done
        assert!(root.join("aaa").exists(), "apagou o vizinho");
        assert!(!root.join("bbb").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lista_vem_do_maior_pro_menor() {
        let root = tmp("ordem");
        index(&root, "peq", 10, true, None);
        index(&root, "gra", 5000, true, None);
        index(&root, "med", 500, true, None);
        let e = scan(&root, &[]);
        assert_eq!(e.iter().map(|x| x.uuid.as_str()).collect::<Vec<_>>(), ["gra", "med", "peq"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn etiqueta_sobrevive_a_ida_e_volta() {
        let root = tmp("roundtrip");
        let l = label("Wikipédia médica (pt) — 2024", "wikimed_pt.zim", 1234, "D:/z/wikimed_pt.zim");
        let dir = index(&root, "k", 1, true, Some(&l));
        assert_eq!(read_label(&dir).unwrap(), l);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn etiqueta_ilegivel_cai_no_balde_conservador() {
        let root = tmp("etiqueta-torta");
        let dir = index(&root, "x", 1, true, None);
        std::fs::write(dir.join(LABEL), "{não é json").unwrap();
        assert!(read_label(&dir).is_none());
        let e = scan(&root, &[]);
        assert!(!e[0].labeled);
        assert!(unrecognized(&e).is_empty(), "etiqueta ilegível virou alvo de limpeza");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pastas_inexistentes_nao_sao_erro() {
        let nada = std::env::temp_dir().join("localzim-nao-existe-mesmo");
        assert_eq!(tree_stats(&nada), (0, 0));
        assert!(scan(&nada, &[]).is_empty());
        assert_eq!(remove_trees(&[]), Freed::default());
    }

    #[test]
    fn limpezas_sao_idempotentes() {
        let root = tmp("idempotente");
        index(&root, "aaa", 100, false, None);
        let alvo: Vec<_> = incomplete(&scan(&root, &[]))
            .iter()
            .filter_map(|u| safe_child(&root, u))
            .collect();
        assert_eq!(remove_trees(&alvo).files, 1);
        assert_eq!(remove_trees(&alvo), Freed::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
