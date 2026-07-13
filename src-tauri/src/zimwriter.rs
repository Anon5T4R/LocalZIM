//! Criador de arquivos ZIM a partir de uma pasta local (estilo zimwriterfs).
//!
//! Escreve o esquema de namespaces clássico (A = artigos, I = recursos,
//! M = metadados, '-' = favicon), clusters zstd para texto e sem compressão
//! para mídia (streaming — arquivo grande não passa inteiro pela RAM),
//! listas de ponteiros ordenadas e md5 no rodapé. O resultado abre no
//! próprio LocalZIM e no Kiwix.

use md5::{Digest, Md5};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAGIC: u32 = 0x044D_495A;
/// Payload máximo de um cluster agrupado (antes da compressão).
const CLUSTER_MAX: usize = 2 * 1024 * 1024;

pub struct CreateSpec {
    pub source: PathBuf,
    pub output: PathBuf,
    pub title: String,
    pub description: String,
    pub language: String,
    pub creator: String,
    /// Página inicial relativa à pasta (None = auto: index.html ou 1º HTML).
    pub main_page: Option<String>,
}

#[derive(Debug)]
pub struct CreateResult {
    pub entries: u32,
    pub articles: u32,
    pub size: u64,
}

enum Payload {
    File(PathBuf, u64),
    Bytes(Vec<u8>),
}

impl Payload {
    fn len(&self) -> u64 {
        match self {
            Payload::File(_, n) => *n,
            Payload::Bytes(b) => b.len() as u64,
        }
    }
}

enum NewKind {
    Content {
        mime_idx: u16,
        payload: Payload,
        cluster: u32,
        blob: u32,
    },
    /// Redirect resolvido por (ns, url) depois da ordenação.
    Redirect { target: (u8, String) },
}

struct NewEntry {
    ns: u8,
    url: String,
    title: String,
    kind: NewKind,
}

fn mime_for(ext: &str) -> (&'static str, bool) {
    // (mime, comprimível)
    match ext {
        "html" | "htm" => ("text/html", true),
        "css" => ("text/css", true),
        "js" | "mjs" => ("application/javascript", true),
        "json" => ("application/json", true),
        "txt" | "md" => ("text/plain", true),
        "xml" => ("text/xml", true),
        "svg" => ("image/svg+xml", true),
        "png" => ("image/png", false),
        "jpg" | "jpeg" => ("image/jpeg", false),
        "gif" => ("image/gif", false),
        "webp" => ("image/webp", false),
        "ico" => ("image/x-icon", false),
        "woff" => ("font/woff", false),
        "woff2" => ("font/woff2", false),
        "ttf" => ("font/ttf", false),
        "otf" => ("font/otf", false),
        "mp4" => ("video/mp4", false),
        "webm" => ("video/webm", false),
        "mp3" => ("audio/mpeg", false),
        "ogg" | "oga" => ("audio/ogg", false),
        "wav" => ("audio/wav", false),
        "pdf" => ("application/pdf", false),
        "wasm" => ("application/wasm", false),
        "zip" => ("application/zip", false),
        _ => ("application/octet-stream", false),
    }
}

/// Extrai o <title> de um HTML (primeiros 32 KB), com fallback.
fn html_title(path: &Path, fallback: &str) -> String {
    let mut buf = vec![0u8; 32 * 1024];
    let n = File::open(path)
        .and_then(|mut f| {
            let mut read = 0;
            while read < buf.len() {
                let k = f.read(&mut buf[read..])?;
                if k == 0 {
                    break;
                }
                read += k;
            }
            Ok(read)
        })
        .unwrap_or(0);
    let low = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
    let src = String::from_utf8_lossy(&buf[..n]).into_owned();
    if let Some(i) = low.find("<title") {
        if let Some(j) = low[i..].find('>') {
            let start = i + j + 1;
            if let Some(k) = low[start..].find("</title") {
                let t = src[start..start + k]
                    .replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&#39;", "'");
                let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if !t.is_empty() {
                    return t;
                }
            }
        }
    }
    fallback.to_string()
}

/// Varre a pasta recursivamente; caminhos relativos com '/', pulando
/// dotfiles/dotdirs (.git etc.) e o próprio arquivo de saída.
fn walk(dir: &Path, base: &Path, output: &Path, out: &mut Vec<(String, PathBuf, u64)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path == output {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&path, base, output, out)?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let size = entry.metadata()?.len();
            out.push((rel, path, size));
        }
    }
    Ok(())
}

struct W(Vec<u8>);
impl W {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn cstr(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }
}

pub fn create(
    spec: &CreateSpec,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f32),
) -> Result<CreateResult, String> {
    let source = spec
        .source
        .canonicalize()
        .map_err(|e| format!("Pasta de origem inválida: {e}"))?;
    let output = spec.output.clone();

    // 1) Enumera os arquivos
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    walk(&source, &source, &output, &mut files).map_err(|e| format!("Falha lendo a pasta: {e}"))?;
    if files.is_empty() {
        return Err("A pasta não tem nenhum arquivo".into());
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    // 2) Monta as entradas
    fn mime_idx(m: &str, mimes: &mut Vec<String>) -> u16 {
        if let Some(i) = mimes.iter().position(|x| x == m) {
            i as u16
        } else {
            mimes.push(m.to_string());
            (mimes.len() - 1) as u16
        }
    }
    let mut mimes: Vec<String> = Vec::new();

    let mut entries: Vec<NewEntry> = Vec::new();
    let mut first_html: Option<String> = None;
    let mut has_index = false;
    let mut favicon_target: Option<String> = None;
    let mut articles = 0u32;

    for (rel, path, size) in &files {
        let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let (mime, _) = mime_for(&ext);
        let is_html = mime == "text/html";
        let ns = if is_html { b'A' } else { b'I' };
        let fname = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let title = if is_html {
            articles += 1;
            if first_html.is_none() {
                first_html = Some(rel.clone());
            }
            if rel == "index.html" {
                has_index = true;
            }
            html_title(path, &fname)
        } else {
            String::new() // título vazio = usa a URL (spec)
        };
        if favicon_target.is_none() && (fname == "favicon.png" || fname == "favicon.ico") {
            favicon_target = Some(rel.clone());
        }
        let mi = mime_idx(mime, &mut mimes);
        entries.push(NewEntry {
            ns,
            url: rel.clone(),
            title,
            kind: NewKind::Content {
                mime_idx: mi,
                payload: Payload::File(path.clone(), *size),
                cluster: 0,
                blob: 0,
            },
        });
    }

    // Metadados (namespace M) — texto puro
    let plain = mime_idx("text/plain", &mut mimes);
    let today = {
        // YYYY-MM-DD sem dependência de chrono: dias desde epoch → data civil
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        civil_from_days((secs / 86400) as i64)
    };
    let meta: Vec<(&str, String)> = vec![
        ("Title", spec.title.trim().to_string()),
        ("Description", spec.description.trim().to_string()),
        ("Language", spec.language.trim().to_string()),
        ("Creator", spec.creator.trim().to_string()),
        ("Publisher", "LocalZIM".into()),
        ("Date", today),
    ];
    for (name, value) in meta {
        if value.is_empty() {
            continue;
        }
        entries.push(NewEntry {
            ns: b'M',
            url: name.to_string(),
            title: String::new(),
            kind: NewKind::Content {
                mime_idx: plain,
                payload: Payload::Bytes(value.into_bytes()),
                cluster: 0,
                blob: 0,
            },
        });
    }
    // Ilustração/favicon: PNG vira também o metadado que a biblioteca mostra
    if let Some(fav) = &favicon_target {
        if fav.ends_with(".png") {
            if let Some((_, path, _)) = files.iter().find(|(r, _, _)| r == fav) {
                if let Ok(bytes) = fs::read(path) {
                    let png = mime_idx("image/png", &mut mimes);
                    entries.push(NewEntry {
                        ns: b'M',
                        url: "Illustration_48x48@1".into(),
                        title: String::new(),
                        kind: NewKind::Content {
                            mime_idx: png,
                            payload: Payload::Bytes(bytes),
                            cluster: 0,
                            blob: 0,
                        },
                    });
                }
            }
        }
        entries.push(NewEntry {
            ns: b'-',
            url: "favicon".into(),
            title: String::new(),
            kind: NewKind::Redirect {
                target: (b'I', fav.clone()),
            },
        });
    }

    // 3) Ordena por (ns, url) e resolve índices
    entries.sort_by(|a, b| (a.ns, a.url.as_bytes()).cmp(&(b.ns, b.url.as_bytes())));
    let find_idx = |ns: u8, url: &str, entries: &[NewEntry]| -> Option<u32> {
        entries
            .binary_search_by(|e| (e.ns, e.url.as_bytes()).cmp(&(ns, url.as_bytes())))
            .ok()
            .map(|i| i as u32)
    };

    // Página principal
    let main_rel = match &spec.main_page {
        Some(p) if !p.trim().is_empty() => p.trim().replace('\\', "/"),
        _ => {
            if has_index {
                "index.html".to_string()
            } else {
                first_html.clone().ok_or("A pasta não tem nenhum arquivo HTML")?
            }
        }
    };
    let main_idx = find_idx(b'A', &main_rel, &entries)
        .ok_or_else(|| format!("Página inicial '{main_rel}' não encontrada na pasta"))?;

    // 4) Atribui clusters: zstd pra texto, cru pra mídia, agrupando até 2 MiB.
    //    A ordem dos blobs dentro do cluster segue a ordem de atribuição.
    #[derive(Default)]
    struct Plan {
        compressed: bool,
        blobs: Vec<usize>,
        payload: u64,
    }
    let mut plans: Vec<Plan> = Vec::new();
    let mut open: HashMap<bool, usize> = HashMap::new(); // comprimível -> plano aberto
    let compressible_mime: Vec<bool> = mimes
        .iter()
        .map(|m| {
            m.starts_with("text/")
                || m.contains("javascript")
                || m.contains("json")
                || m.contains("xml")
        })
        .collect();
    for i in 0..entries.len() {
        let (mi, len) = match &entries[i].kind {
            NewKind::Content { mime_idx, payload, .. } => (*mime_idx as usize, payload.len()),
            _ => continue,
        };
        // Offsets de blob são u32: um arquivo não pode passar de ~4 GiB.
        if len > u32::MAX as u64 - 64 {
            return Err(format!(
                "O arquivo '{}' passa de 4 GiB — grande demais para um cluster ZIM",
                entries[i].url
            ));
        }
        let comp = compressible_mime[mi];
        let plan_idx = match open.get(&comp) {
            Some(&p) if plans[p].payload + len <= CLUSTER_MAX as u64 => p,
            _ => {
                plans.push(Plan { compressed: comp, ..Default::default() });
                let p = plans.len() - 1;
                open.insert(comp, p);
                p
            }
        };
        let blob = plans[plan_idx].blobs.len() as u32;
        plans[plan_idx].blobs.push(i);
        plans[plan_idx].payload += len;
        if let NewKind::Content { cluster, blob: b, .. } = &mut entries[i].kind {
            *cluster = plan_idx as u32;
            *b = blob;
        }
    }

    // 5) Cabeça do arquivo em memória (header + mimes + dirents + ponteiros)
    let mut dirents: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for e in &entries {
        let mut w = W(Vec::new());
        match &e.kind {
            NewKind::Content { mime_idx, cluster, blob, .. } => {
                w.u16(*mime_idx);
                w.0.push(0);
                w.0.push(e.ns);
                w.u32(0);
                w.u32(*cluster);
                w.u32(*blob);
            }
            NewKind::Redirect { target } => {
                let t = find_idx(target.0, &target.1, &entries)
                    .ok_or_else(|| format!("Alvo de redirect '{}' sumiu", target.1))?;
                w.u16(0xffff);
                w.0.push(0);
                w.0.push(e.ns);
                w.u32(0);
                w.u32(t);
            }
        }
        w.cstr(&e.url);
        w.cstr(&e.title);
        dirents.push(w.0);
    }

    let mut mime_blob = Vec::new();
    for m in &mimes {
        mime_blob.extend_from_slice(m.as_bytes());
        mime_blob.push(0);
    }
    mime_blob.push(0);

    let mime_pos = 80u64;
    let mut dirent_pos = Vec::with_capacity(dirents.len());
    let mut off = mime_pos + mime_blob.len() as u64;
    for d in &dirents {
        dirent_pos.push(off);
        off += d.len() as u64;
    }
    let url_ptr_pos = off;
    let title_ptr_pos = url_ptr_pos + 8 * entries.len() as u64;
    let cluster_ptr_pos = title_ptr_pos + 4 * entries.len() as u64;
    let clusters_start = cluster_ptr_pos + 8 * plans.len() as u64;

    // Lista de títulos: índices ordenados por (ns, título-ou-url)
    let mut title_order: Vec<u32> = (0..entries.len() as u32).collect();
    title_order.sort_by(|&a, &b| {
        let ea = &entries[a as usize];
        let eb = &entries[b as usize];
        let ta = if ea.title.is_empty() { &ea.url } else { &ea.title };
        let tb = if eb.title.is_empty() { &eb.url } else { &eb.title };
        (ea.ns, ta.as_bytes()).cmp(&(eb.ns, tb.as_bytes()))
    });

    let mut head = W(Vec::with_capacity(clusters_start as usize));
    head.u32(MAGIC);
    head.u16(6); // major
    head.u16(0); // minor: esquema clássico
    // UUID: md5 do caminho de saída + data (estável o suficiente, sem dep nova)
    let uuid: [u8; 16] = Md5::digest(format!("{}|{}", output.display(), spec.title).as_bytes()).into();
    head.0.extend_from_slice(&uuid);
    head.u32(entries.len() as u32);
    head.u32(plans.len() as u32);
    head.u64(url_ptr_pos);
    head.u64(title_ptr_pos);
    head.u64(cluster_ptr_pos);
    head.u64(mime_pos);
    head.u32(main_idx);
    head.u32(0xffff_ffff); // layout page (obsoleto)
    head.u64(0); // checksum_pos — backfill
    debug_assert_eq!(head.0.len(), 80);
    head.0.extend_from_slice(&mime_blob);
    for d in &dirents {
        head.0.extend_from_slice(d);
    }
    for p in &dirent_pos {
        head.u64(*p);
    }
    for t in &title_order {
        head.u32(*t);
    }
    for _ in &plans {
        head.u64(0); // ponteiros de cluster — backfill
    }

    // 6) Escreve: cabeça + clusters (streaming) + backfill + md5
    if let Some(parent) = output.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut f = File::create(&output).map_err(|e| format!("Falha ao criar o arquivo: {e}"))?;
    f.write_all(&head.0).map_err(|e| e.to_string())?;

    let total_blobs: usize = plans.iter().map(|p| p.blobs.len()).sum();
    let mut done_blobs = 0usize;
    let mut cluster_offsets: Vec<u64> = Vec::with_capacity(plans.len());

    for plan in &plans {
        if cancel.load(Ordering::Relaxed) {
            drop(f);
            let _ = fs::remove_file(&output);
            return Err("Criação cancelada".into());
        }
        cluster_offsets.push(f.stream_position().map_err(|e| e.to_string())?);
        // Tabela de offsets (u32) calculada só com os tamanhos
        let n = plan.blobs.len();
        let mut offs = W(Vec::with_capacity(4 * (n + 1)));
        let mut acc = (4 * (n + 1)) as u32;
        offs.u32(acc);
        for &ei in &plan.blobs {
            if let NewKind::Content { payload, .. } = &entries[ei].kind {
                acc += payload.len() as u32;
                offs.u32(acc);
            }
        }
        if plan.compressed {
            f.write_all(&[0x05]).map_err(|e| e.to_string())?; // zstd
            let mut enc =
                zstd::stream::Encoder::new(&mut f, 9).map_err(|e| e.to_string())?;
            enc.write_all(&offs.0).map_err(|e| e.to_string())?;
            for &ei in &plan.blobs {
                if let NewKind::Content { payload, .. } = &entries[ei].kind {
                    write_payload(&mut enc, payload)?;
                    done_blobs += 1;
                }
            }
            enc.finish().map_err(|e| e.to_string())?;
        } else {
            f.write_all(&[0x01]).map_err(|e| e.to_string())?; // cru
            f.write_all(&offs.0).map_err(|e| e.to_string())?;
            for &ei in &plan.blobs {
                if let NewKind::Content { payload, .. } = &entries[ei].kind {
                    write_payload(&mut f, payload)?;
                    done_blobs += 1;
                }
            }
        }
        on_progress(done_blobs as f32 / total_blobs.max(1) as f32);
    }

    let checksum_pos = f.stream_position().map_err(|e| e.to_string())?;
    // Backfill: checksum_pos no header + ponteiros de cluster
    f.seek(SeekFrom::Start(72)).map_err(|e| e.to_string())?;
    f.write_all(&checksum_pos.to_le_bytes()).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(cluster_ptr_pos)).map_err(|e| e.to_string())?;
    for c in &cluster_offsets {
        f.write_all(&c.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);

    // md5 de todo o conteúdo até checksum_pos, anexado no fim
    let mut rf = File::open(&output).map_err(|e| e.to_string())?;
    let mut hasher = Md5::new();
    let mut remaining = checksum_pos;
    let mut buf = vec![0u8; 1 << 20];
    while remaining > 0 {
        let take = buf.len().min(remaining as usize);
        let k = rf.read(&mut buf[..take]).map_err(|e| e.to_string())?;
        if k == 0 {
            break;
        }
        hasher.update(&buf[..k]);
        remaining -= k as u64;
    }
    drop(rf);
    let digest = hasher.finalize();
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(&output)
        .map_err(|e| e.to_string())?;
    f.write_all(&digest).map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();

    Ok(CreateResult {
        entries: entries.len() as u32,
        articles,
        size,
    })
}

fn write_payload(dst: &mut impl Write, payload: &Payload) -> Result<(), String> {
    match payload {
        Payload::Bytes(b) => dst.write_all(b).map_err(|e| e.to_string()),
        Payload::File(path, _) => {
            let mut src = File::open(path).map_err(|e| format!("Falha lendo {}: {e}", path.display()))?;
            io::copy(&mut src, dst).map(|_| ()).map_err(|e| e.to_string())
        }
    }
}

/// Conversão dias-desde-epoch → (ano, mês, dia) — algoritmo de Howard Hinnant.
fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zim::ZimFile;

    fn setup_site(dir: &Path) {
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(
            dir.join("index.html"),
            "<html><head><title>Meu Site</title></head><body>Home <a href=\"sub/pagina.html\">p</a></body></html>",
        )
        .unwrap();
        fs::write(
            dir.join("sub/pagina.html"),
            "<html><head><title>Página Dois</title></head><body>Conteúdo interno</body></html>",
        )
        .unwrap();
        fs::write(dir.join("estilo.css"), "body{color:red}").unwrap();
        fs::write(dir.join("favicon.png"), b"\x89PNG-fake").unwrap();
        // dotdir deve ser ignorado
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git/config"), "x").unwrap();
    }

    #[test]
    fn cria_zim_de_pasta_e_reabre_com_o_leitor() {
        let base = std::env::temp_dir().join(format!("localzim-writer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("site");
        setup_site(&src);
        let out = base.join("meu-site.zim");

        let spec = CreateSpec {
            source: src,
            output: out.clone(),
            title: "Meu Site".into(),
            description: "Teste do criador".into(),
            language: "por".into(),
            creator: "João".into(),
            main_page: None,
        };
        let cancel = AtomicBool::new(false);
        let mut progressed = false;
        let res = create(&spec, &cancel, |_| progressed = true).unwrap();
        assert!(progressed);
        assert_eq!(res.articles, 2);

        // Round-trip: o próprio parser do LocalZIM lê o arquivo criado
        let z = ZimFile::open(&out).unwrap();
        assert_eq!(z.main_path().unwrap(), "A/index.html");
        let (mime, body) = z
            .content(&z.find(b'A', "sub/pagina.html").unwrap().unwrap().1)
            .unwrap()
            .unwrap();
        assert_eq!(mime, "text/html");
        assert!(String::from_utf8_lossy(&body).contains("Conteúdo interno"));

        // CSS em cluster zstd, favicon em cluster cru
        let (mime, body) = z
            .content(&z.find(b'I', "estilo.css").unwrap().unwrap().1)
            .unwrap()
            .unwrap();
        assert_eq!(mime, "text/css");
        assert_eq!(body, b"body{color:red}");

        // metadados + título extraído do HTML + dotdir ignorado
        assert_eq!(z.meta_string("Title").unwrap(), "Meu Site");
        assert!(z.find(b'I', ".git/config").unwrap().is_none());
        let sug = z.suggest("Página", 5).unwrap();
        assert!(sug.iter().any(|(t, _)| t == "Página Dois"));

        // favicon: redirect '-' + ilustração M
        assert!(z.favicon().is_some());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn cancelamento_apaga_o_arquivo_parcial() {
        let base = std::env::temp_dir().join(format!("localzim-writer-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("site");
        setup_site(&src);
        let out = base.join("x.zim");
        let spec = CreateSpec {
            source: src,
            output: out.clone(),
            title: "X".into(),
            description: String::new(),
            language: "por".into(),
            creator: String::new(),
            main_page: None,
        };
        let cancel = AtomicBool::new(true); // cancela antes do 1º cluster
        let err = create(&spec, &cancel, |_| {}).unwrap_err();
        assert!(err.contains("cancelada"));
        assert!(!out.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn datas_civis_convertem_certo() {
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(19_723), "2024-01-01");
    }
}
