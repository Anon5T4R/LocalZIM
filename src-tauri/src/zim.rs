//! Leitor de arquivos ZIM (openzim.org) em Rust puro — sem libzim/C++.
//!
//! Formato: https://wiki.openzim.org/wiki/ZIM_file_format
//! Suporta clusters sem compressão, LZMA2/XZ e Zstandard; o esquema de
//! namespaces antigo (A/I/M/-, minorVersion 0) e o novo (C/M/W/X, minor >= 1).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAGIC: u32 = 0x044D_495A; // "ZIM\x04"
const MAX_REDIRECTS: u32 = 12;
/// Trava de segurança por cluster descomprimido (1 GiB).
const MAX_DECOMPRESSED: u64 = 1 << 30;
const CLUSTER_CACHE_CAP: usize = 12;

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

#[derive(Debug, Clone)]
pub struct Header {
    /// Guardado por completude do formato; a lógica só depende do `minor`.
    #[allow(dead_code)]
    pub major: u16,
    pub minor: u16,
    pub uuid: [u8; 16],
    pub entry_count: u32,
    pub cluster_count: u32,
    pub url_ptr_pos: u64,
    pub title_ptr_pos: u64,
    pub cluster_ptr_pos: u64,
    pub mime_list_pos: u64,
    pub main_page: u32,
    pub checksum_pos: u64,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Content { mime: u16, cluster: u32, blob: u32 },
    Redirect { target: u32 },
    /// linktarget / deleted — sem conteúdo servível.
    Other,
}

#[derive(Debug, Clone)]
pub struct Dirent {
    pub namespace: u8,
    pub url: String,
    pub title: String,
    pub kind: Kind,
}

impl Dirent {
    /// Título de exibição (spec: título vazio = usar a URL).
    pub fn title_or_url(&self) -> &str {
        if self.title.is_empty() {
            &self.url
        } else {
            &self.title
        }
    }

    /// Caminho servível "N/url" (namespace + URL).
    pub fn entry_path(&self) -> String {
        format!("{}/{}", self.namespace as char, self.url)
    }
}

/// Conteúdo de um cluster já descomprimido (tabela de offsets + blobs).
struct ClusterData {
    extended: bool,
    data: Vec<u8>,
}

pub struct ZimFile {
    file: Mutex<File>,
    pub size: u64,
    pub header: Header,
    mime_types: Vec<String>,
    /// Índice de títulos só de artigos (X/listing/titleOrdered/v1), se existir.
    article_titles: Option<Vec<u32>>,
    cluster_cache: Mutex<(VecDeque<u32>, HashMap<u32, Arc<ClusterData>>)>,
}

impl ZimFile {
    pub fn open(path: &Path) -> io::Result<ZimFile> {
        let mut f = File::open(path)?;
        let size = f.metadata()?.len();
        let mut hb = [0u8; 80];
        f.read_exact(&mut hb)
            .map_err(|_| bad("arquivo pequeno demais para ser um ZIM"))?;
        if u32le(&hb, 0) != MAGIC {
            return Err(bad("não é um arquivo ZIM (assinatura inválida)"));
        }
        let header = Header {
            major: u16le(&hb, 4),
            minor: u16le(&hb, 6),
            uuid: hb[8..24].try_into().unwrap(),
            entry_count: u32le(&hb, 24),
            cluster_count: u32le(&hb, 28),
            url_ptr_pos: u64le(&hb, 32),
            title_ptr_pos: u64le(&hb, 40),
            cluster_ptr_pos: u64le(&hb, 48),
            mime_list_pos: u64le(&hb, 56),
            main_page: u32le(&hb, 64),
            checksum_pos: u64le(&hb, 72),
        };

        // Lista de MIME: strings terminadas em NUL, encerrada por uma vazia.
        f.seek(SeekFrom::Start(header.mime_list_pos))?;
        let mut raw = Vec::new();
        f.by_ref().take(1 << 16).read_to_end(&mut raw)?;
        let mut mime_types = Vec::new();
        let mut i = 0usize;
        while i < raw.len() {
            let Some(p) = raw[i..].iter().position(|&b| b == 0) else {
                break;
            };
            if p == 0 {
                break; // string vazia = fim da lista
            }
            mime_types.push(String::from_utf8_lossy(&raw[i..i + p]).into_owned());
            i += p + 1;
        }

        let mut zim = ZimFile {
            file: Mutex::new(f),
            size,
            header,
            mime_types,
            article_titles: None,
            cluster_cache: Mutex::new((VecDeque::new(), HashMap::new())),
        };
        zim.article_titles = zim.load_article_titles();
        Ok(zim)
    }

    // ---------- leitura de baixo nível ----------

    fn read_at(&self, pos: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut f = self.file.lock().unwrap();
        f.seek(SeekFrom::Start(pos))?;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Lê até `len` bytes (sem erro se bater no fim do arquivo).
    fn read_at_most(&self, pos: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut f = self.file.lock().unwrap();
        f.seek(SeekFrom::Start(pos))?;
        let mut buf = Vec::with_capacity(len.min(1 << 20));
        f.by_ref().take(len as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }

    // ---------- dirents ----------

    fn dirent_at(&self, pos: u64) -> io::Result<Dirent> {
        let mut chunk = 512usize;
        loop {
            let buf = self.read_at_most(pos, chunk)?;
            if let Some(d) = Self::parse_dirent(&buf) {
                return Ok(d);
            }
            if pos + (buf.len() as u64) >= self.size || chunk >= 1 << 20 {
                return Err(bad("dirent truncado"));
            }
            chunk *= 4;
        }
    }

    /// Devolve None se o buffer não contém o dirent completo (precisa ler mais).
    fn parse_dirent(buf: &[u8]) -> Option<Dirent> {
        if buf.len() < 12 {
            return None;
        }
        let mime = u16le(buf, 0);
        let namespace = buf[3];
        let (kind, mut off) = match mime {
            0xffff => (
                Kind::Redirect {
                    target: u32le(buf, 8),
                },
                12usize,
            ),
            0xfffe | 0xfffd => (Kind::Other, 8),
            m => {
                if buf.len() < 16 {
                    return None;
                }
                (
                    Kind::Content {
                        mime: m,
                        cluster: u32le(buf, 8),
                        blob: u32le(buf, 12),
                    },
                    16,
                )
            }
        };
        let url_end = off + buf[off..].iter().position(|&b| b == 0)?;
        let url = String::from_utf8_lossy(&buf[off..url_end]).into_owned();
        off = url_end + 1;
        let title_end = off + buf.get(off..)?.iter().position(|&b| b == 0)?;
        let title = String::from_utf8_lossy(&buf[off..title_end]).into_owned();
        Some(Dirent {
            namespace,
            url,
            title,
            kind,
        })
    }

    /// Entrada pelo índice na lista de ponteiros de URL.
    pub fn entry_at(&self, idx: u32) -> io::Result<Dirent> {
        if idx >= self.header.entry_count {
            return Err(bad("índice de entrada fora do arquivo"));
        }
        let p = u64le(
            &self.read_at(self.header.url_ptr_pos + 8 * idx as u64, 8)?,
            0,
        );
        self.dirent_at(p)
    }

    /// Entrada pela posição na lista de títulos do cabeçalho.
    fn entry_by_title_pos(&self, i: u32) -> io::Result<(u32, Dirent)> {
        if i >= self.header.entry_count {
            return Err(bad("índice de título fora do arquivo"));
        }
        let e = u32le(
            &self.read_at(self.header.title_ptr_pos + 4 * i as u64, 4)?,
            0,
        );
        Ok((e, self.entry_at(e)?))
    }

    /// Busca binária por (namespace, url) na lista ordenada de URLs.
    pub fn find(&self, namespace: u8, url: &str) -> io::Result<Option<(u32, Dirent)>> {
        let mut lo = 0i64;
        let mut hi = self.header.entry_count as i64 - 1;
        while lo <= hi {
            let mid = ((lo + hi) / 2) as u32;
            let d = self.entry_at(mid)?;
            match (d.namespace, d.url.as_bytes()).cmp(&(namespace, url.as_bytes())) {
                std::cmp::Ordering::Equal => return Ok(Some((mid, d))),
                std::cmp::Ordering::Less => lo = mid as i64 + 1,
                std::cmp::Ordering::Greater => hi = mid as i64 - 1,
            }
        }
        Ok(None)
    }

    /// Segue cadeias de redirecionamento até uma entrada final.
    pub fn follow(&self, mut d: Dirent) -> io::Result<Dirent> {
        for _ in 0..MAX_REDIRECTS {
            match d.kind {
                Kind::Redirect { target } => d = self.entry_at(target)?,
                _ => return Ok(d),
            }
        }
        Err(bad("cadeia de redirecionamentos longa demais"))
    }

    // ---------- clusters e blobs ----------

    /// Identificador estável do arquivo (UUID do header em hex).
    pub fn uuid_hex(&self) -> String {
        self.header.uuid.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn cluster_range(&self, idx: u32) -> io::Result<(u64, u64)> {
        if idx >= self.header.cluster_count {
            return Err(bad("cluster fora do arquivo"));
        }
        let start = u64le(
            &self.read_at(self.header.cluster_ptr_pos + 8 * idx as u64, 8)?,
            0,
        );
        let end = if idx + 1 < self.header.cluster_count {
            u64le(
                &self.read_at(self.header.cluster_ptr_pos + 8 * (idx as u64 + 1), 8)?,
                0,
            )
        } else {
            self.header.checksum_pos
        };
        let end = end.min(self.size);
        if end <= start {
            return Err(bad("cluster vazio ou corrompido"));
        }
        Ok((start, end))
    }

    fn cluster(&self, idx: u32) -> io::Result<Arc<ClusterData>> {
        {
            let cache = self.cluster_cache.lock().unwrap();
            if let Some(c) = cache.1.get(&idx) {
                return Ok(c.clone());
            }
        }
        let (start, end) = self.cluster_range(idx)?;
        let raw = self.read_at(start, (end - start) as usize)?;
        let comp = raw[0] & 0x0f;
        let extended = raw[0] & 0x10 != 0;
        let body = &raw[1..];
        let data = match comp {
            0 | 1 => body.to_vec(),
            4 => {
                let mut out = Vec::new();
                xz2::read::XzDecoder::new(body)
                    .take(MAX_DECOMPRESSED)
                    .read_to_end(&mut out)?;
                out
            }
            5 => {
                let mut out = Vec::new();
                zstd::stream::Decoder::new(body)?
                    .take(MAX_DECOMPRESSED)
                    .read_to_end(&mut out)?;
                out
            }
            other => return Err(bad(format!("compressão de cluster não suportada: {other}"))),
        };
        let c = Arc::new(ClusterData { extended, data });
        let mut cache = self.cluster_cache.lock().unwrap();
        if !cache.1.contains_key(&idx) {
            if cache.0.len() >= CLUSTER_CACHE_CAP {
                if let Some(old) = cache.0.pop_front() {
                    cache.1.remove(&old);
                }
            }
            cache.0.push_back(idx);
            cache.1.insert(idx, c.clone());
        }
        Ok(c)
    }

    pub fn blob(&self, cluster: u32, blob: u32) -> io::Result<Vec<u8>> {
        // Cluster sem compressão: lê só a fatia do blob direto do arquivo,
        // sem carregar o cluster inteiro (importante para vídeos grandes).
        let (start, end) = self.cluster_range(cluster)?;
        let head = self.read_at(start, 1)?[0];
        if head & 0x0f <= 1 {
            let extended = head & 0x10 != 0;
            let osz: u64 = if extended { 8 } else { 4 };
            let base = start + 1;
            let read_off = |i: u64| -> io::Result<u64> {
                let o = base + i * osz;
                if o + osz > end {
                    return Err(bad("offset de blob fora do cluster"));
                }
                Ok(if extended {
                    u64le(&self.read_at(o, 8)?, 0)
                } else {
                    u32le(&self.read_at(o, 4)?, 0) as u64
                })
            };
            let first = read_off(0)?;
            let count = (first / osz).saturating_sub(1);
            if blob as u64 >= count {
                return Err(bad("blob fora do cluster"));
            }
            let a = read_off(blob as u64)?;
            let b = read_off(blob as u64 + 1)?;
            if b < a || base + b > end {
                return Err(bad("blob corrompido"));
            }
            return self.read_at(base + a, (b - a) as usize);
        }

        let c = self.cluster(cluster)?;
        let osz = if c.extended { 8usize } else { 4 };
        let get = |i: usize| -> io::Result<u64> {
            let o = i * osz;
            if o + osz > c.data.len() {
                return Err(bad("offset de blob fora do cluster"));
            }
            Ok(if c.extended {
                u64le(&c.data, o)
            } else {
                u32le(&c.data, o) as u64
            })
        };
        let first = get(0)? as usize;
        let count = (first / osz).saturating_sub(1);
        if blob as usize >= count {
            return Err(bad("blob fora do cluster"));
        }
        let a = get(blob as usize)? as usize;
        let b = get(blob as usize + 1)? as usize;
        if b < a || b > c.data.len() {
            return Err(bad("blob corrompido"));
        }
        Ok(c.data[a..b].to_vec())
    }

    /// Conteúdo (mime, bytes) de uma entrada de conteúdo; None para redirect/other.
    pub fn content(&self, d: &Dirent) -> io::Result<Option<(String, Vec<u8>)>> {
        match d.kind {
            Kind::Content {
                mime,
                cluster,
                blob,
            } => {
                let m = self
                    .mime_types
                    .get(mime as usize)
                    .cloned()
                    .unwrap_or_else(|| "application/octet-stream".into());
                Ok(Some((m, self.blob(cluster, blob)?)))
            }
            _ => Ok(None),
        }
    }

    pub fn is_html(&self, d: &Dirent) -> bool {
        match d.kind {
            Kind::Content { mime, .. } => self
                .mime_types
                .get(mime as usize)
                .map(|m| m.starts_with("text/html"))
                .unwrap_or(false),
            _ => false,
        }
    }

    // ---------- semântica de alto nível ----------

    /// Namespace onde vivem os artigos: 'C' no esquema novo, 'A' no antigo.
    pub fn article_namespace(&self) -> u8 {
        if self.header.minor >= 1 {
            b'C'
        } else {
            b'A'
        }
    }

    fn load_article_titles(&self) -> Option<Vec<u32>> {
        let (_, d) = self.find(b'X', "listing/titleOrdered/v1").ok()??;
        let d = self.follow(d).ok()?;
        let (_, bytes) = self.content(&d).ok()??;
        let mut v = Vec::with_capacity(bytes.len() / 4);
        for ch in bytes.chunks_exact(4) {
            v.push(u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]));
        }
        Some(v)
    }

    pub fn article_count(&self) -> Option<u32> {
        self.article_titles.as_ref().map(|l| l.len() as u32)
    }

    fn title_count(&self) -> u32 {
        match &self.article_titles {
            Some(l) => l.len() as u32,
            None => self.header.entry_count,
        }
    }

    fn article_by_title_pos(&self, i: u32) -> io::Result<(u32, Dirent)> {
        match &self.article_titles {
            Some(list) => {
                let e = *list
                    .get(i as usize)
                    .ok_or_else(|| bad("índice de título fora da lista"))?;
                Ok((e, self.entry_at(e)?))
            }
            None => self.entry_by_title_pos(i),
        }
    }

    /// Metadado do namespace M (Title, Description, Language, Illustration…).
    pub fn meta(&self, name: &str) -> Option<Vec<u8>> {
        let (_, d) = self.find(b'M', name).ok()??;
        let d = self.follow(d).ok()?;
        Some(self.content(&d).ok()??.1)
    }

    pub fn meta_string(&self, name: &str) -> Option<String> {
        self.meta(name)
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Caminho "N/url" da página principal.
    pub fn main_path(&self) -> Option<String> {
        if self.header.main_page != u32::MAX {
            if let Ok(d) = self.entry_at(self.header.main_page) {
                if let Ok(d) = self.follow(d) {
                    return Some(d.entry_path());
                }
            }
        }
        if let Ok(Some((_, d))) = self.find(b'W', "mainPage") {
            if let Ok(d) = self.follow(d) {
                return Some(d.entry_path());
            }
        }
        // Fallback: primeiro artigo HTML pelo índice de títulos.
        for i in 0..self.title_count().min(64) {
            if let Ok((_, d)) = self.article_by_title_pos(i) {
                if let Ok(d) = self.follow(d) {
                    if self.is_html(&d) {
                        return Some(d.entry_path());
                    }
                }
            }
        }
        None
    }

    /// Ícone do arquivo: metadado Illustration (novo) ou entrada favicon (antigo).
    pub fn favicon(&self) -> Option<(String, Vec<u8>)> {
        for name in ["Illustration_48x48@1", "Illustration_48x48", "Favicon"] {
            if let Some(b) = self.meta(name) {
                if !b.is_empty() {
                    return Some(("image/png".into(), b));
                }
            }
        }
        for (ns, url) in [(b'-', "favicon"), (b'I', "favicon"), (b'I', "favicon.png")] {
            if let Ok(Some((_, d))) = self.find(ns, url) {
                if let Ok(d) = self.follow(d) {
                    if let Ok(Some((m, b))) = self.content(&d) {
                        return Some((m, b));
                    }
                }
            }
        }
        None
    }

    /// Sugestões por prefixo de título (case-sensitive, com variantes de
    /// capitalização — a busca full-text Xapian do ZIM fica fora do escopo).
    /// Devolve pares (título, caminho "N/url").
    pub fn suggest(&self, query: &str, limit: usize) -> io::Result<Vec<(String, String)>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        let mut variants = vec![query.to_string()];
        for v in [
            capitalize_first(query),
            title_case(query),
            query.to_lowercase(),
        ] {
            if !variants.contains(&v) {
                variants.push(v);
            }
        }

        let ans = self.article_namespace();
        let n = self.title_count();
        let mut out: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();

        for v in &variants {
            if out.len() >= limit {
                break;
            }
            // lower bound na lista de títulos ordenada
            let mut lo = 0u32;
            let mut hi = n;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let (_, d) = self.article_by_title_pos(mid)?;
                let less = if self.article_titles.is_some() {
                    d.title_or_url() < v.as_str()
                } else {
                    (d.namespace, d.title_or_url()) < (ans, v.as_str())
                };
                if less {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            let mut i = lo;
            while i < n && out.len() < limit {
                let (e, d) = self.article_by_title_pos(i)?;
                if self.article_titles.is_none() && d.namespace != ans {
                    break;
                }
                let t = d.title_or_url();
                if !t.starts_with(v.as_str()) {
                    break;
                }
                let servable = matches!(d.kind, Kind::Redirect { .. }) || self.is_html(&d);
                if servable && seen.insert(e) {
                    out.push((t.to_string(), d.entry_path()));
                }
                i += 1;
            }
        }
        Ok(out)
    }

    /// Caminho de um artigo HTML aleatório.
    pub fn random_article(&self) -> Option<String> {
        let n = self.title_count() as u64;
        if n == 0 {
            return None;
        }
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .subsec_nanos() as u64
            ^ 0x9E37_79B9_7F4A_7C15;
        for _ in 0..256 {
            // xorshift simples — só precisa espalhar, não ser criptográfico
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let i = (seed % n) as u32;
            let Ok((_, d)) = self.article_by_title_pos(i) else {
                continue;
            };
            let Ok(d) = self.follow(d) else { continue };
            if d.namespace == self.article_namespace() && self.is_html(&d) {
                return Some(d.entry_path());
            }
        }
        None
    }
}

fn capitalize_first(s: &str) -> String {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::path::PathBuf;

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
        fn bytes(&mut self, b: &[u8]) {
            self.0.extend_from_slice(b);
        }
        fn cstr(&mut self, s: &str) {
            self.0.extend_from_slice(s.as_bytes());
            self.0.push(0);
        }
    }

    fn content_dirent(mime: u16, ns: u8, cluster: u32, blob: u32, url: &str, title: &str) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.u16(mime);
        w.0.push(0); // parameter len
        w.0.push(ns);
        w.u32(0); // revision
        w.u32(cluster);
        w.u32(blob);
        w.cstr(url);
        w.cstr(title);
        w.0
    }

    fn redirect_dirent(ns: u8, target: u32, url: &str, title: &str) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.u16(0xffff);
        w.0.push(0);
        w.0.push(ns);
        w.u32(0);
        w.u32(target);
        w.cstr(url);
        w.cstr(title);
        w.0
    }

    /// Monta o payload de um cluster (tabela de offsets u32 + blobs).
    fn cluster_payload(blobs: &[&[u8]]) -> Vec<u8> {
        let n = blobs.len();
        let mut off = (4 * (n + 1)) as u32;
        let mut w = W(Vec::new());
        w.u32(off);
        for b in blobs {
            off += b.len() as u32;
            w.u32(off);
        }
        for b in blobs {
            w.bytes(b);
        }
        w.0
    }

    const HTML_MAIN: &[u8] =
        b"<html><head><title>Main</title></head><body>Ola <a href=\"Other.html\">x</a></body></html>";
    const PNG_FAKE: &[u8] = b"\x89PNG-fake-image-bytes";
    const META_TITLE: &[u8] = b"Wiki Teste";

    /// Monta um ZIM sintético (esquema antigo, minor 0) com:
    ///   0 A/Main.html  "Main Page"  html   cluster0(zstd) blob0
    ///   1 A/Other.html "Outra"      redirect -> 0
    ///   2 I/logo.png   ""           png    cluster1(raw)  blob0
    ///   3 M/Title      ""           plain  cluster0(zstd) blob1
    fn build_zim() -> Vec<u8> {
        let mimes: &[u8] = b"text/html\0image/png\0text/plain\0\0";

        let dirents = [
            content_dirent(0, b'A', 0, 0, "Main.html", "Main Page"),
            redirect_dirent(b'A', 0, "Other.html", "Outra"),
            content_dirent(1, b'I', 1, 0, "logo.png", ""),
            content_dirent(2, b'M', 0, 1, "Title", ""),
        ];
        // ordem por (ns, título): Main Page, Outra, logo.png, Title
        let title_order: [u32; 4] = [0, 1, 2, 3];

        let cluster0 = {
            let payload = cluster_payload(&[HTML_MAIN, META_TITLE]);
            let mut c = vec![0x05u8]; // zstd
            c.extend_from_slice(&zstd::encode_all(&payload[..], 3).unwrap());
            c
        };
        let cluster1 = {
            let mut c = vec![0x01u8]; // sem compressão
            c.extend_from_slice(&cluster_payload(&[PNG_FAKE]));
            c
        };

        // layout: header | mimes | dirents | url_ptrs | title_ptrs | cluster_ptrs | clusters | checksum
        let mime_pos = 80u64;
        let mut dirent_pos = Vec::new();
        let mut off = mime_pos + mimes.len() as u64;
        for d in &dirents {
            dirent_pos.push(off);
            off += d.len() as u64;
        }
        let url_ptr_pos = off;
        let title_ptr_pos = url_ptr_pos + 8 * dirents.len() as u64;
        let cluster_ptr_pos = title_ptr_pos + 4 * dirents.len() as u64;
        let cluster0_pos = cluster_ptr_pos + 8 * 2;
        let cluster1_pos = cluster0_pos + cluster0.len() as u64;
        let checksum_pos = cluster1_pos + cluster1.len() as u64;

        let mut w = W(Vec::new());
        w.u32(MAGIC);
        w.u16(6); // major
        w.u16(0); // minor: esquema antigo
        w.bytes(&[0u8; 16]); // uuid
        w.u32(dirents.len() as u32);
        w.u32(2); // clusters
        w.u64(url_ptr_pos);
        w.u64(title_ptr_pos);
        w.u64(cluster_ptr_pos);
        w.u64(mime_pos);
        w.u32(0); // main page = entrada 0
        w.u32(0xffff_ffff); // layout page (obsoleto)
        w.u64(checksum_pos);
        assert_eq!(w.0.len(), 80);

        w.bytes(mimes);
        for d in &dirents {
            w.bytes(d);
        }
        for p in &dirent_pos {
            w.u64(*p);
        }
        for t in &title_order {
            w.u32(*t);
        }
        w.u64(cluster0_pos);
        w.u64(cluster1_pos);
        w.bytes(&cluster0);
        w.bytes(&cluster1);
        w.bytes(&[0u8; 16]); // md5 (não verificado)
        w.0
    }

    fn temp_zim(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("localzim-test-{}-{}.zim", std::process::id(), tag));
        let mut f = File::create(&p).unwrap();
        f.write_all(&build_zim()).unwrap();
        p
    }

    #[test]
    fn abre_e_navega_o_zim_sintetico() {
        let path = temp_zim("navega");
        let z = ZimFile::open(&path).unwrap();

        assert_eq!(z.header.entry_count, 4);
        assert_eq!(z.article_namespace(), b'A');
        assert!(z.article_titles.is_none());

        // busca binária por URL + conteúdo em cluster zstd
        let (_, d) = z.find(b'A', "Main.html").unwrap().unwrap();
        assert!(z.is_html(&d));
        let (mime, body) = z.content(&d).unwrap().unwrap();
        assert_eq!(mime, "text/html");
        assert_eq!(body, HTML_MAIN);

        // redirect segue até o artigo
        let (_, r) = z.find(b'A', "Other.html").unwrap().unwrap();
        assert!(matches!(r.kind, Kind::Redirect { .. }));
        let alvo = z.follow(r).unwrap();
        assert_eq!(alvo.url, "Main.html");

        // cluster sem compressão
        let (mime, body) = z
            .content(&z.find(b'I', "logo.png").unwrap().unwrap().1)
            .unwrap()
            .unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(body, PNG_FAKE);

        // metadados e página principal
        assert_eq!(z.meta_string("Title").unwrap(), "Wiki Teste");
        assert_eq!(z.main_path().unwrap(), "A/Main.html");

        // entrada inexistente
        assert!(z.find(b'A', "Nada").unwrap().is_none());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sugestoes_por_prefixo_com_variantes_de_caixa() {
        let path = temp_zim("sugestoes");
        let z = ZimFile::open(&path).unwrap();

        let s = z.suggest("Ma", 10).unwrap();
        assert_eq!(s[0].0, "Main Page");
        assert_eq!(s[0].1, "A/Main.html");

        // minúsculas acham pela variante capitalizada
        let s = z.suggest("main", 10).unwrap();
        assert!(s.iter().any(|(t, _)| t == "Main Page"));

        // redirects entram como sugestão
        let s = z.suggest("Ou", 10).unwrap();
        assert!(s.iter().any(|(t, _)| t == "Outra"));

        // prefixo sem resultado
        assert!(z.suggest("Zzz", 10).unwrap().is_empty());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn artigo_aleatorio_devolve_html() {
        let path = temp_zim("aleatorio");
        let z = ZimFile::open(&path).unwrap();
        let p = z.random_article().unwrap();
        assert_eq!(p, "A/Main.html");
        std::fs::remove_file(&path).ok();
    }
}
