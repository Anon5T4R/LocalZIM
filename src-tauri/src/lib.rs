//! LocalZIM — leitor de bibliotecas ZIM (Kiwix) offline.
//!
//! O conteúdo do arquivo ZIM é servido ao webview pelo protocolo customizado
//! `zim://` (no Windows vira `http://zim.localhost/`): `/<id>/<N>/<url>`, onde
//! `id` identifica o arquivo aberto e `N/url` é o caminho da entrada no ZIM.

mod crawler;
mod search;
mod storage;
mod translate;
mod zim;
mod zimwriter;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use serde::Serialize;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use zim::{Kind, ZimFile};

const BRIDGE: &str = include_str!("bridge.js");

/// Escape mínimo para segmentos de path de URL.
const PATH_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'{')
    .add(b'}');

struct OpenBook {
    file: Arc<ZimFile>,
    info: ZimInfo,
}

#[derive(Default)]
struct AppState {
    books: Mutex<HashMap<String, OpenBook>>,
    /// Indexações full-text em andamento, por id de livro.
    ft_builds: Mutex<HashMap<String, Arc<search::FtBuild>>>,
    /// Índices full-text já abertos, por id de livro.
    ft_indexes: Mutex<HashMap<String, Arc<search::FtIndex>>>,
    /// Criação de ZIM em andamento (uma por vez); o flag cancela.
    zim_create: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Modelos de tradução (downloads, engines carregadas na RAM).
    translator: translate::Translator,
}

fn get_book(state: &AppState, id: &str) -> Result<Arc<ZimFile>, String> {
    state
        .books
        .lock()
        .unwrap()
        .get(id)
        .map(|b| b.file.clone())
        .ok_or_else(|| "arquivo ZIM não está aberto".into())
}

/// Pasta do índice full-text de um arquivo (chaveada pelo UUID do ZIM).
fn ft_dir(app: &tauri::AppHandle, zf: &ZimFile) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("fulltext")
        .join(zf.uuid_hex()))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ZimInfo {
    id: String,
    path: String,
    file_name: String,
    name: String,
    description: String,
    language: String,
    creator: String,
    date: String,
    entry_count: u32,
    article_count: Option<u32>,
    size: u64,
    main_path: Option<String>,
    favicon: Option<String>,
}

#[derive(Serialize)]
struct Suggestion {
    title: String,
    path: String,
}

// ---------- comandos ----------

#[tauri::command]
async fn open_zim(state: State<'_, AppState>, path: String) -> Result<ZimInfo, String> {
    let pb = PathBuf::from(&path);
    let canon = pb.canonicalize().unwrap_or_else(|_| pb.clone());
    let mut h = DefaultHasher::new();
    canon.to_string_lossy().to_lowercase().hash(&mut h);
    let id = format!("{:016x}", h.finish());

    if let Some(b) = state.books.lock().unwrap().get(&id) {
        return Ok(b.info.clone());
    }

    let zf = ZimFile::open(&pb).map_err(|e| format!("Falha ao abrir o arquivo ZIM: {e}"))?;
    let name = zf
        .meta_string("Title")
        .unwrap_or_else(|| {
            pb.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "ZIM".into())
        });
    let favicon = zf
        .favicon()
        .map(|(m, b)| format!("data:{};base64,{}", m, B64.encode(b)));
    let info = ZimInfo {
        id: id.clone(),
        path: pb.to_string_lossy().into_owned(),
        file_name: pb
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        name,
        description: zf.meta_string("Description").unwrap_or_default(),
        language: zf.meta_string("Language").unwrap_or_default(),
        creator: zf.meta_string("Creator").unwrap_or_default(),
        date: zf.meta_string("Date").unwrap_or_default(),
        entry_count: zf.header.entry_count,
        article_count: zf.article_count(),
        size: zf.size,
        main_path: zf.main_path(),
        favicon,
    };
    state.books.lock().unwrap().insert(
        id,
        OpenBook {
            file: Arc::new(zf),
            info: info.clone(),
        },
    );
    Ok(info)
}

#[tauri::command]
fn close_zim(state: State<'_, AppState>, id: String) {
    state.books.lock().unwrap().remove(&id);
}

#[tauri::command]
async fn zim_suggest(
    state: State<'_, AppState>,
    id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<Suggestion>, String> {
    let file = get_book(&state, &id)?;
    let out = file
        .suggest(&query, limit.unwrap_or(12).min(50))
        .map_err(|e| e.to_string())?;
    Ok(out
        .into_iter()
        .map(|(title, path)| Suggestion { title, path })
        .collect())
}

#[tauri::command]
async fn zim_random(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    Ok(get_book(&state, &id)?.random_article())
}

// ---------- busca full-text ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FtStatus {
    state: String, // none | building | ready
    progress: f32,
    docs: Option<u64>,
}

#[tauri::command]
async fn fulltext_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<FtStatus, String> {
    if let Some(b) = state.ft_builds.lock().unwrap().get(&id) {
        return Ok(FtStatus {
            state: "building".into(),
            progress: b.progress.load(Ordering::Relaxed) as f32 / 1000.0,
            docs: None,
        });
    }
    let file = get_book(&state, &id)?;
    let dir = ft_dir(&app, &file)?;
    if let Some(docs) = search::is_ready(&dir) {
        return Ok(FtStatus {
            state: "ready".into(),
            progress: 1.0,
            docs: Some(docs),
        });
    }
    Ok(FtStatus {
        state: "none".into(),
        progress: 0.0,
        docs: None,
    })
}

#[tauri::command]
async fn fulltext_build(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let file = get_book(&state, &id)?;
    let st = {
        let mut builds = state.ft_builds.lock().unwrap();
        if builds.contains_key(&id) {
            return Ok(()); // já está indexando
        }
        let st = Arc::new(search::FtBuild::new());
        builds.insert(id.clone(), st.clone());
        st
    };
    // índice antigo aberto (se houver) fica inválido
    state.ft_indexes.lock().unwrap().remove(&id);
    let dir = ft_dir(&app, &file)?;
    // Etiqueta o índice com o ZIM que o gera. A pasta é nomeada pelo uuid, do
    // qual não dá pra voltar ao livro — sem isto o painel de armazenamento não
    // teria como dizer de quem é o índice nem reconhecer o arquivo depois que
    // ele muda de pasta. Best-effort: falhar aqui não pode derrubar a
    // indexação. Escrita ANTES do build porque o build começa apagando a pasta
    // e a recriando; o write_label roda depois, no fim, junto do done.
    let rotulo = {
        let books = state.books.lock().unwrap();
        books.get(&id).map(|b| storage::Label {
            name: b.info.name.clone(),
            file_name: b.info.file_name.clone(),
            size: b.info.size,
            path: b.info.path.clone(),
        })
    };

    std::thread::spawn(move || {
        let _ = app.emit("fulltext", json!({ "id": id, "state": "building", "progress": 0.0 }));
        let res = search::build(&file, &dir, &st, |p| {
            let _ = app.emit("fulltext", json!({ "id": id, "state": "building", "progress": p }));
        });
        // Só etiqueta se o índice ficou pronto: pasta interrompida tem que
        // permanecer no balde "incompleto", que é o de risco zero.
        if res.is_ok() {
            if let Some(l) = &rotulo {
                storage::write_label(&dir, l);
            }
        }
        app.state::<AppState>().ft_builds.lock().unwrap().remove(&id);
        let payload = match res {
            Ok(docs) => json!({ "id": id, "state": "ready", "progress": 1.0, "docs": docs }),
            Err(e) => json!({ "id": id, "state": "error", "progress": 0.0, "error": e }),
        };
        let _ = app.emit("fulltext", payload);
    });
    Ok(())
}

#[tauri::command]
fn fulltext_cancel(state: State<'_, AppState>, id: String) {
    if let Some(b) = state.ft_builds.lock().unwrap().get(&id) {
        b.cancel.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
async fn fulltext_search(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<search::FtHit>, String> {
    let file = get_book(&state, &id)?;
    let ft = {
        let mut idxs = state.ft_indexes.lock().unwrap();
        match idxs.get(&id) {
            Some(f) => f.clone(),
            None => {
                let dir = ft_dir(&app, &file)?;
                if search::is_ready(&dir).is_none() {
                    return Err("o índice de busca ainda não foi construído".into());
                }
                let f = Arc::new(search::open(&dir).map_err(|e| e.to_string())?);
                idxs.insert(id.clone(), f.clone());
                f
            }
        }
    };
    search::search(&ft, &file, &query, limit.unwrap_or(20))
}

// ---------- tradução offline ----------

fn app_data(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
async fn translate_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    direction: String,
) -> Result<translate::DirectionStatus, String> {
    let dir = app_data(&app)?;
    translate::direction_status(&dir, &state.translator, &direction)
        .ok_or_else(|| format!("direção não suportada: {direction}"))
}

/// Baixa um modelo (uma "perna") em thread própria; progresso via evento
/// "translate-model" ({leg, state: downloading|ready|error, received, total}).
#[tauri::command]
async fn translate_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    leg: String,
) -> Result<(), String> {
    let Some(cancel) = translate::start_download(&state.translator, &leg) else {
        return Ok(()); // já está baixando
    };
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let dir = match app_data(&app) {
            Ok(d) => d,
            Err(e) => {
                translate::finish_download(&app.state::<AppState>().translator, &leg);
                let _ = app.emit("translate-model", json!({ "leg": leg, "state": "error", "error": e }));
                return;
            }
        };
        let res = translate::download_leg(&dir, &leg, &cancel, |received, total| {
            // sem spam de eventos: no máximo ~7 por segundo
            if last.elapsed().as_millis() >= 150 || received == total {
                last = std::time::Instant::now();
                let _ = app.emit(
                    "translate-model",
                    json!({ "leg": leg, "state": "downloading", "received": received, "total": total }),
                );
            }
        });
        translate::finish_download(&app.state::<AppState>().translator, &leg);
        let payload = match res {
            Ok(()) => json!({ "leg": leg, "state": "ready" }),
            Err(e) if e == "cancelado" => json!({ "leg": leg, "state": "cancelled" }),
            Err(e) => json!({ "leg": leg, "state": "error", "error": e }),
        };
        let _ = app.emit("translate-model", payload);
    });
    Ok(())
}

#[tauri::command]
fn translate_cancel_download(state: State<'_, AppState>, leg: String) {
    translate::cancel_download(&state.translator, &leg);
}

#[tauri::command]
async fn translate_remove(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    leg: String,
) -> Result<(), String> {
    let dir = app_data(&app)?;
    translate::remove_leg(&dir, &state.translator, &leg)
}

/// Carrega o(s) modelo(s) da direção na RAM (a UI mostra "carregando modelo"
/// enquanto isso, separado do progresso de tradução).
#[tauri::command]
async fn translate_prepare(app: tauri::AppHandle, direction: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app_data(&app)?;
        translate::prepare(&dir, &app.state::<AppState>().translator, &direction)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Traduz um lote de blocos de texto de um artigo (com cache em disco).
#[tauri::command]
async fn translate_texts(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    article: String,
    direction: String,
    texts: Vec<String>,
) -> Result<Vec<String>, String> {
    let uuid = get_book(&state, &id)?.uuid_hex();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app_data(&app)?;
        translate::translate_texts(
            &dir,
            &app.state::<AppState>().translator,
            &uuid,
            &article,
            &direction,
            texts,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Arquivo .zim passado na linha de comando (clique duplo / associação).
#[tauri::command]
fn startup_file() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|a| a.to_lowercase().ends_with(".zim"))
}

// ---------- criação de ZIM a partir de pasta ----------

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateZimSpec {
    source: String,
    output: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    creator: String,
    #[serde(default)]
    main_page: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CreateZimResult {
    entries: u32,
    articles: u32,
    size: u64,
    output: String,
}

/// Empacota uma pasta num .zim, em thread própria, com progresso via evento
/// "zim-create" ({state: building|done|error, progress, ...}).
#[tauri::command]
async fn create_zim(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    spec: CreateZimSpec,
) -> Result<(), String> {
    let cancel = {
        let mut slot = state.zim_create.lock().unwrap();
        if slot.is_some() {
            return Err("Já existe uma criação de ZIM em andamento".into());
        }
        let c = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *slot = Some(c.clone());
        c
    };
    std::thread::spawn(move || {
        let app2 = app.clone();
        // panic aqui dentro não pode deixar o slot preso "em andamento" pra sempre
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let ws = zimwriter::CreateSpec {
                source: PathBuf::from(&spec.source),
                output: PathBuf::from(&spec.output),
                title: spec.title.clone(),
                description: spec.description.clone(),
                language: if spec.language.trim().is_empty() {
                    "por".into()
                } else {
                    spec.language.clone()
                },
                creator: spec.creator.clone(),
                main_page: spec.main_page.clone(),
            };
            let _ = app.emit("zim-create", json!({ "state": "building", "progress": 0.0 }));
            let res = zimwriter::create(&ws, &cancel, |p| {
                let _ = app.emit("zim-create", json!({ "state": "building", "progress": p }));
            });
            *app.state::<AppState>().zim_create.lock().unwrap() = None;
            let payload = match res {
                Ok(r) => json!({
                    "state": "done",
                    "progress": 1.0,
                    "result": CreateZimResult {
                        entries: r.entries,
                        articles: r.articles,
                        size: r.size,
                        output: spec.output,
                    },
                }),
                Err(e) => json!({ "state": "error", "progress": 0.0, "error": e }),
            };
            let _ = app.emit("zim-create", payload);
        }));
        if ran.is_err() {
            *app2.state::<AppState>().zim_create.lock().unwrap() = None;
            let _ = app2.emit(
                "zim-create",
                json!({ "state": "error", "progress": 0.0, "error": "Erro interno inesperado durante a criação" }),
            );
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_create_zim(state: State<'_, AppState>) {
    if let Some(c) = state.zim_create.lock().unwrap().as_ref() {
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrawlZimSpec {
    url: String,
    output: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    creator: String,
    #[serde(default)]
    max_depth: Option<u32>,
    #[serde(default)]
    max_pages: Option<u32>,
    #[serde(default)]
    delay_ms: Option<u64>,
    /// Só páginas dentro do diretório da URL inicial (padrão: sim).
    #[serde(default)]
    same_path: Option<bool>,
}

/// Baixa um site (crawler estático, mesmo host, robots.txt respeitado) e
/// empacota num .zim. Mesmo canal de eventos "zim-create", com `phase`:
/// "crawl" (contagem de páginas) e "pack" (progresso do empacotamento).
#[tauri::command]
async fn create_zim_from_site(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    spec: CrawlZimSpec,
) -> Result<(), String> {
    let cancel = {
        let mut slot = state.zim_create.lock().unwrap();
        if slot.is_some() {
            return Err("Já existe uma criação de ZIM em andamento".into());
        }
        let c = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *slot = Some(c.clone());
        c
    };
    std::thread::spawn(move || {
        let app_guard = app.clone();
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let done = |app: &tauri::AppHandle| {
            *app.state::<AppState>().zim_create.lock().unwrap() = None;
        };
        let staging = std::env::temp_dir().join(format!(
            "localzim-crawl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));

        let cspec = crawler::CrawlSpec {
            start_url: spec.url.clone(),
            max_depth: spec.max_depth.unwrap_or(3),
            max_pages: spec.max_pages.unwrap_or(200),
            delay_ms: spec.delay_ms.unwrap_or(200),
            same_path: spec.same_path.unwrap_or(true),
        };
        let max_pages = cspec.max_pages.max(1);
        let _ = app.emit(
            "zim-create",
            json!({ "state": "building", "phase": "crawl", "progress": 0.0, "pages": 0 }),
        );
        let outcome = crawler::crawl(&cspec, &staging, &cancel, |p| {
            let _ = app.emit(
                "zim-create",
                json!({
                    "state": "building",
                    "phase": "crawl",
                    "progress": (p.pages as f32 / max_pages as f32).min(1.0),
                    "pages": p.pages,
                    "files": p.files,
                    "queued": p.queued,
                }),
            );
        });
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                let _ = fs_remove_dir(&staging);
                done(&app);
                let _ = app.emit("zim-create", json!({ "state": "error", "progress": 0.0, "error": e }));
                return;
            }
        };

        let ws = zimwriter::CreateSpec {
            source: staging.clone(),
            output: PathBuf::from(&spec.output),
            title: spec.title.clone(),
            description: spec.description.clone(),
            language: if spec.language.trim().is_empty() {
                "por".into()
            } else {
                spec.language.clone()
            },
            creator: spec.creator.clone(),
            main_page: Some(outcome.main_local.clone()),
        };
        let res = zimwriter::create(&ws, &cancel, |p| {
            let _ = app.emit(
                "zim-create",
                json!({ "state": "building", "phase": "pack", "progress": p }),
            );
        });
        let _ = fs_remove_dir(&staging);
        done(&app);
        let payload = match res {
            Ok(r) => json!({
                "state": "done",
                "progress": 1.0,
                "result": CreateZimResult {
                    entries: r.entries,
                    articles: r.articles,
                    size: r.size,
                    output: spec.output,
                },
            }),
            Err(e) => json!({ "state": "error", "progress": 0.0, "error": e }),
        };
        let _ = app.emit("zim-create", payload);
        }));
        if ran.is_err() {
            *app_guard.state::<AppState>().zim_create.lock().unwrap() = None;
            let _ = app_guard.emit(
                "zim-create",
                json!({ "state": "error", "progress": 0.0, "error": "Erro interno inesperado durante a criação" }),
            );
        }
    });
    Ok(())
}

fn fs_remove_dir(p: &std::path::Path) -> std::io::Result<()> {
    std::fs::remove_dir_all(p)
}

// ---------- protocolo zim:// ----------

fn encode_entry_path(p: &str) -> String {
    p.split('/')
        .map(|s| utf8_percent_encode(s, PATH_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn http_error(status: u16, msg: &str) -> tauri::http::Response<Vec<u8>> {
    let body = format!(
        "<!doctype html><html lang=\"pt-BR\"><meta charset=\"utf-8\"><body style=\"font-family:sans-serif;color:#555;text-align:center;padding-top:15vh\"><h2>{status}</h2><p>{msg}</p><p><a href=\"javascript:history.back()\" style=\"color:#7c3aed\">&larr; Voltar</a></p></body></html>"
    );
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(body.into_bytes())
        .unwrap()
}

/// Injeta a ponte logo depois de `<head...>` (ou no início, se não houver).
fn inject_bridge(body: &[u8]) -> Vec<u8> {
    fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len())
            .position(|w| w.eq_ignore_ascii_case(needle))
    }
    let tag = format!("<script>{BRIDGE}</script>");
    let at = find_ci(body, b"<head")
        .and_then(|i| body[i..].iter().position(|&b| b == b'>').map(|j| i + j + 1))
        .unwrap_or(0);
    let mut out = Vec::with_capacity(body.len() + tag.len());
    out.extend_from_slice(&body[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&body[at..]);
    out
}

/// "bytes=a-b" → (início, fim) inclusivos, validados contra `len`.
fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let first = h.trim().strip_prefix("bytes=")?.split(',').next()?;
    let (a, b) = first.split_once('-')?;
    if a.is_empty() {
        let suffix: u64 = b.trim().parse().ok()?;
        if suffix == 0 {
            return None;
        }
        Some((len.saturating_sub(suffix), len - 1))
    } else {
        let start: u64 = a.trim().parse().ok()?;
        let end: u64 = if b.trim().is_empty() {
            len - 1
        } else {
            b.trim().parse().ok()?
        };
        if start > end || start >= len {
            return None;
        }
        Some((start, end.min(len - 1)))
    }
}

fn serve(
    state: &AppState,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri();
    let path = uri.path().trim_start_matches('/');
    let Some((id, entry_raw)) = path.split_once('/') else {
        return http_error(404, "Caminho sem identificador do arquivo.");
    };
    let entry = percent_decode_str(entry_raw).decode_utf8_lossy().into_owned();

    let file = state.books.lock().unwrap().get(id).map(|b| b.file.clone());
    let Some(file) = file else {
        return http_error(404, "Este arquivo ZIM não está mais aberto no LocalZIM.");
    };

    // raiz do livro → página principal
    let entry = if entry.is_empty() {
        match file.main_path() {
            Some(p) => p,
            None => return http_error(404, "O arquivo não declara uma página principal."),
        }
    } else {
        entry
    };

    let Some((ns, url)) = entry.split_once('/') else {
        return http_error(404, "Caminho de entrada inválido.");
    };
    if ns.len() != 1 {
        return http_error(404, "Namespace inválido.");
    }
    let ns = ns.as_bytes()[0];

    let dirent = match file.find(ns, url) {
        Ok(Some((_, d))) => d,
        Ok(None) => return http_error(404, "Página não encontrada neste arquivo ZIM."),
        Err(e) => return http_error(500, &format!("Erro lendo o arquivo: {e}")),
    };

    // Redirecionamento vira 302 — a URL nova mantém a resolução de links relativos.
    if let Kind::Redirect { target } = dirent.kind {
        return match file.entry_at(target).and_then(|d| file.follow(d)) {
            Ok(d) => {
                let loc = format!("/{}/{}", id, encode_entry_path(&d.entry_path()));
                tauri::http::Response::builder()
                    .status(302)
                    .header("Location", loc)
                    .body(Vec::new())
                    .unwrap()
            }
            Err(e) => http_error(500, &format!("Redirecionamento quebrado: {e}")),
        };
    }

    match file.content(&dirent) {
        Ok(Some((mime, body))) => {
            let is_html = mime.starts_with("text/html");
            let body = if is_html { inject_bridge(&body) } else { body };
            let ct = if mime.starts_with("text/") && !mime.contains("charset") {
                format!("{mime}; charset=utf-8")
            } else {
                mime
            };
            // Range (vídeo/áudio com seek) — só para conteúdo que não é HTML.
            if !is_html {
                let range = request
                    .headers()
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|h| parse_range(h, body.len() as u64));
                if let Some((s, e)) = range {
                    let total = body.len() as u64;
                    let slice = body[s as usize..=e as usize].to_vec();
                    return tauri::http::Response::builder()
                        .status(206)
                        .header("Content-Type", ct)
                        .header("Accept-Ranges", "bytes")
                        .header("Content-Range", format!("bytes {s}-{e}/{total}"))
                        .header("Cache-Control", "public, max-age=3600")
                        .body(slice)
                        .unwrap();
                }
            }
            tauri::http::Response::builder()
                .status(200)
                .header("Content-Type", ct)
                .header("Accept-Ranges", "bytes")
                .header("Cache-Control", "public, max-age=3600")
                .body(body)
                .unwrap()
        }
        Ok(None) => http_error(404, "Entrada sem conteúdo servível."),
        Err(e) => http_error(500, &format!("Erro extraindo o conteúdo: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn intervalos_de_range_validos_e_invalidos() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-200", 1000), Some((800, 999)));
        assert_eq!(parse_range("bytes=0-99,200-300", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-2000", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-2", 1000), None);
        assert_eq!(parse_range("lixo", 1000), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }
}

// ---------- bootstrap ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        if let Some(f) = args
            .iter()
            .skip(1)
            .find(|a| a.to_lowercase().ends_with(".zim"))
        {
            let _ = app.emit("open-file", f.clone());
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            // modelos de tradução saem da RAM depois de um tempo sem uso
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                handle.state::<AppState>().translator.evict_idle();
            });
            Ok(())
        })
        .register_uri_scheme_protocol("zim", |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            serve(&state, &request)
        })
        .invoke_handler(tauri::generate_handler![
            open_zim,
            close_zim,
            zim_suggest,
            zim_random,
            startup_file,
            fulltext_status,
            fulltext_build,
            fulltext_cancel,
            fulltext_search,
            create_zim,
            create_zim_from_site,
            cancel_create_zim,
            translate_status,
            translate_download,
            translate_cancel_download,
            translate_remove,
            translate_prepare,
            translate_texts,
            storage::storage_info,
            storage::storage_clear_incomplete,
            storage::storage_delete_index,
            storage::storage_clear_translate_cache
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o LocalZIM");
}
