//! Crawler estático local — "Criar .zim de um site".
//!
//! BFS a partir de uma URL: páginas HTML só do mesmo host (com limite de
//! profundidade e de páginas), assets (CSS/JS/imagens/fontes/mídia) de
//! qualquer host, respeitando o robots.txt do site e com intervalo educado
//! entre requisições. Tudo vai para uma pasta de staging com links
//! reescritos para caminhos relativos — que o `zimwriter` empacota.
//!
//! Limite conhecido (documentado na UI): páginas montadas por JavaScript
//! (SPA) saem como o HTML cru do servidor; para essas, o zimit é a ferramenta
//! certa.

use lol_html::html_content::ContentType;
use lol_html::{element, text, RewriteStrSettings};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::blocking::Client;
use reqwest::Url;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Teto de segurança por arquivo baixado (200 MiB).
const MAX_FILE: u64 = 200 * 1024 * 1024;
const UA: &str = "LocalZIM/0.4 (+https://github.com/Anon5T4R/LocalZIM)";

pub struct CrawlSpec {
    pub start_url: String,
    pub max_depth: u32,
    pub max_pages: u32,
    pub delay_ms: u64,
}

pub struct CrawlOutcome {
    /// Caminho local (relativo ao staging) da página inicial.
    pub main_local: String,
    pub pages: u32,
    pub files: u32,
}

#[derive(Clone)]
pub struct CrawlProgress {
    pub pages: u32,
    pub files: u32,
    pub queued: u32,
    pub current: String,
}

// ---------------------------------------------------------------------------
// Mapeamento URL → caminho local
// ---------------------------------------------------------------------------

fn short_hash(s: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

/// Limpa um segmento pra ser nome de arquivo válido no Windows.
fn sanitize_seg(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned == ".." || cleaned == "." {
        "_".into()
    } else {
        cleaned
    }
}

/// Caminho local determinístico de uma URL. Páginas de outros hosts não são
/// baixadas; assets de fora vão pra `_ext/<host>/…`.
pub fn url_to_path(u: &Url, is_html: bool, main_host: &str) -> String {
    let host = u.host_str().unwrap_or("site").to_lowercase();
    let decoded = percent_decode_str(u.path()).decode_utf8_lossy().into_owned();
    let mut path = decoded.trim_start_matches('/').to_string();
    if path.is_empty() || path.ends_with('/') {
        path.push_str("index.html");
    }
    let mut segs: Vec<String> = path
        .split('/')
        .map(sanitize_seg)
        .filter(|s| !s.is_empty())
        .collect();
    let mut fname = segs.pop().unwrap_or_else(|| "index.html".into());
    if let Some(q) = u.query() {
        if !q.is_empty() {
            let h = format!("_{}", short_hash(q));
            match fname.rfind('.') {
                Some(dot) => fname.insert_str(dot, &h),
                None => fname.push_str(&h),
            }
        }
    }
    let lower = fname.to_lowercase();
    if is_html && !lower.ends_with(".html") && !lower.ends_with(".htm") {
        fname.push_str(".html");
    }
    segs.push(fname);
    let rel = segs.join("/");
    if host == main_host {
        rel
    } else {
        format!("_ext/{}/{}", sanitize_seg(&host), rel)
    }
}

/// Caminho relativo de `from` até `to` (ambos relativos ao staging).
pub fn rel_path(from: &str, to: &str) -> String {
    let f: Vec<&str> = from.split('/').collect();
    let t: Vec<&str> = to.split('/').collect();
    let fdir = &f[..f.len() - 1];
    let mut i = 0;
    while i < fdir.len() && i + 1 < t.len() && fdir[i] == t[i] {
        i += 1;
    }
    let mut out = String::new();
    for _ in i..fdir.len() {
        out.push_str("../");
    }
    out.push_str(&t[i..].join("/"));
    out
}

/// Escape mínimo pra usar o caminho relativo dentro de href/src.
const HREF_SET: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'\'').add(b'#').add(b'?').add(b'%');
fn href_encode(p: &str) -> String {
    utf8_percent_encode(p, HREF_SET).to_string()
}

// ---------------------------------------------------------------------------
// robots.txt (parser mínimo: grupos User-agent * / localzim)
// ---------------------------------------------------------------------------

pub fn parse_robots(text: &str) -> Vec<String> {
    let mut applies = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        let low = l.to_ascii_lowercase();
        if let Some(v) = low.strip_prefix("user-agent:") {
            let ua = v.trim();
            applies = ua == "*" || ua.contains("localzim");
        } else if applies {
            if let Some(v) = low.strip_prefix("disallow:") {
                let v = l[l.len() - v.trim_start().len()..].trim().to_string();
                if !v.is_empty() {
                    out.push(v);
                }
            }
        }
    }
    out
}

fn robots_allows(disallow: &[String], path: &str) -> bool {
    !disallow.iter().any(|d| path.starts_with(d.as_str()))
}

// ---------------------------------------------------------------------------
// Extração de links e reescrita
// ---------------------------------------------------------------------------

fn resolve(base: &Url, raw: &str) -> Option<Url> {
    let raw = raw.trim();
    if raw.is_empty()
        || raw.starts_with('#')
        || raw.starts_with("javascript:")
        || raw.starts_with("mailto:")
        || raw.starts_with("data:")
        || raw.starts_with("tel:")
    {
        return None;
    }
    let mut u = base.join(raw).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.set_fragment(None);
    Some(u)
}

/// Entradas de um srcset ("a.png 1x, b.png 2x") → URLs.
fn srcset_urls(base: &Url, srcset: &str) -> Vec<Url> {
    srcset
        .split(',')
        .filter_map(|part| part.trim().split_whitespace().next())
        .filter_map(|u| resolve(base, u))
        .collect()
}

/// Coleta (páginas, assets) de um HTML.
pub fn extract_links(html: &str, base: &Url) -> (Vec<Url>, Vec<Url>) {
    let pages = RefCell::new(Vec::new());
    let assets = RefCell::new(Vec::new());
    let push_asset = |el: &lol_html::html_content::Element, attr: &str| {
        if let Some(v) = el.get_attribute(attr) {
            if let Some(u) = resolve(base, &v) {
                assets.borrow_mut().push(u);
            }
        }
    };
    let _ = lol_html::rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("a[href]", |el| {
                    if let Some(u) = resolve(base, &el.get_attribute("href").unwrap_or_default()) {
                        pages.borrow_mut().push(u);
                    }
                    Ok(())
                }),
                element!("img[src], script[src], source[src], video[src], audio[src], embed[src], iframe[src]", |el| {
                    push_asset(el, "src");
                    Ok(())
                }),
                element!("img[srcset], source[srcset]", |el| {
                    if let Some(ss) = el.get_attribute("srcset") {
                        assets.borrow_mut().extend(srcset_urls(base, &ss));
                    }
                    Ok(())
                }),
                element!("video[poster]", |el| {
                    push_asset(el, "poster");
                    Ok(())
                }),
                element!("link[href]", |el| {
                    let rel = el.get_attribute("rel").unwrap_or_default().to_lowercase();
                    if rel.contains("stylesheet") || rel.contains("icon") || rel.contains("preload") {
                        push_asset(el, "href");
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    );
    (pages.into_inner(), assets.into_inner())
}

/// Reescreve `url(...)` e `@import "..."` de um CSS pra caminhos relativos.
pub fn rewrite_css(css: &str, base: &Url, from_local: &str, map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    let lower = css.to_ascii_lowercase();
    while i < n {
        if lower[i..].starts_with("url(") {
            out.push_str(&css[i..i + 4]);
            i += 4;
            let end = match css[i..].find(')') {
                Some(e) => i + e,
                None => n,
            };
            let raw = css[i..end].trim().trim_matches(['"', '\'']);
            out.push_str(&map_ref(raw, base, from_local, map));
            out.push_str(&css[end..end.min(n)]);
            i = end;
            if i < n {
                out.push(')');
                i += 1;
            }
        } else if lower[i..].starts_with("@import ") {
            out.push_str(&css[i..i + 8]);
            i += 8;
            // @import "x.css";  (a forma url(...) cai no ramo de cima)
            if i < n && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i] as char;
                out.push(quote);
                i += 1;
                let end = css[i..].find(quote).map(|e| i + e).unwrap_or(n);
                out.push_str(&map_ref(&css[i..end], base, from_local, map));
                i = end;
            }
        } else {
            let ch = css[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// URL de referência → caminho relativo local (se baixado) ou original.
fn map_ref(raw: &str, base: &Url, from_local: &str, map: &HashMap<String, String>) -> String {
    match resolve(base, raw) {
        Some(u) => match map.get(u.as_str()) {
            Some(local) => href_encode(&rel_path(from_local, local)),
            None => raw.to_string(),
        },
        None => raw.to_string(),
    }
}

/// Reescreve os links de um HTML: internos viram relativos, externos ficam
/// absolutos (o leitor manda pro navegador), `<base>` some.
pub fn rewrite_html(
    html: &str,
    base: &Url,
    from_local: &str,
    map: &HashMap<String, String>,
) -> String {
    // fragmento preservado nos <a>: resolve sem fragmento, reanexa depois
    let rewrite_attr = |el: &mut lol_html::html_content::Element, attr: &str| {
        let Some(v) = el.get_attribute(attr) else { return };
        let frag = v.find('#').map(|i| v[i..].to_string());
        if let Some(u) = resolve(base, &v) {
            if let Some(local) = map.get(u.as_str()) {
                let mut newv = href_encode(&rel_path(from_local, local));
                if let Some(f) = frag {
                    newv.push_str(&f);
                }
                let _ = el.set_attribute(attr, &newv);
            }
        }
    };
    let style_buf = RefCell::new(String::new());
    lol_html::rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("base", |el| {
                    el.remove();
                    Ok(())
                }),
                element!("a[href], link[href]", |el| {
                    rewrite_attr(el, "href");
                    Ok(())
                }),
                element!(
                    "img[src], script[src], source[src], video[src], audio[src], embed[src], iframe[src]",
                    |el| {
                        rewrite_attr(el, "src");
                        Ok(())
                    }
                ),
                element!("video[poster]", |el| {
                    rewrite_attr(el, "poster");
                    Ok(())
                }),
                element!("img[srcset], source[srcset]", |el| {
                    if let Some(ss) = el.get_attribute("srcset") {
                        let newss: Vec<String> = ss
                            .split(',')
                            .map(|part| {
                                let part = part.trim();
                                let mut it = part.split_whitespace();
                                let u = it.next().unwrap_or("");
                                let rest: Vec<&str> = it.collect();
                                let mapped = map_ref(u, base, from_local, map);
                                if rest.is_empty() {
                                    mapped
                                } else {
                                    format!("{} {}", mapped, rest.join(" "))
                                }
                            })
                            .collect();
                        let _ = el.set_attribute("srcset", &newss.join(", "));
                    }
                    Ok(())
                }),
                text!("style", |t| {
                    style_buf.borrow_mut().push_str(t.as_str());
                    if t.last_in_text_node() {
                        let css = style_buf.borrow_mut().split_off(0);
                        t.replace(&rewrite_css(&css, base, from_local, map), ContentType::Text);
                    } else {
                        t.remove();
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .unwrap_or_else(|_| html.to_string())
}

// ---------------------------------------------------------------------------
// O crawl em si
// ---------------------------------------------------------------------------

enum Kind {
    Page(u32), // profundidade
    Asset,
}

pub fn crawl(
    spec: &CrawlSpec,
    staging: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(CrawlProgress),
) -> Result<CrawlOutcome, String> {
    // usuário digita "site.com" sem esquema o tempo todo — completa sozinho
    let raw = spec.start_url.trim();
    let raw = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let start = Url::parse(&raw).map_err(|e| format!("URL inválida: {e}"))?;
    if start.scheme() != "http" && start.scheme() != "https" {
        return Err("A URL precisa começar com http:// ou https://".into());
    }
    let main_host = start.host_str().ok_or("URL sem host")?.to_lowercase();
    let max_pages = spec.max_pages.clamp(1, 5000);
    let max_depth = spec.max_depth.min(10);
    let delay = Duration::from_millis(spec.delay_ms.min(5000));

    let client = Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // robots.txt do host principal (melhor esforço)
    let disallow: Vec<String> = start
        .join("/robots.txt")
        .ok()
        .and_then(|u| client.get(u).send().ok())
        .filter(|r| r.status().is_success())
        .and_then(|r| r.text().ok())
        .map(|t| parse_robots(&t))
        .unwrap_or_default();

    fs::create_dir_all(staging).map_err(|e| e.to_string())?;

    let mut queue: VecDeque<(Url, Kind)> = VecDeque::new();
    let mut seen: HashSet<String> = HashSet::new();
    // URL final (sem fragmento) → caminho local
    let mut map: HashMap<String, String> = HashMap::new();
    let mut used_paths: HashSet<String> = HashSet::new();
    // (local, url base) dos html/css pra fase de reescrita
    let mut htmls: Vec<(String, Url)> = Vec::new();
    let mut csses: Vec<(String, Url)> = Vec::new();
    let mut pages = 0u32;
    let mut files = 0u32;
    let mut main_local: Option<String> = None;

    seen.insert(start.as_str().to_string());
    queue.push_back((start.clone(), Kind::Page(0)));

    while let Some((url, kind)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Criação cancelada".into());
        }
        let is_page = matches!(kind, Kind::Page(_));
        if is_page && pages >= max_pages {
            continue;
        }
        if url.host_str().map(|h| h.to_lowercase()) == Some(main_host.clone())
            && !robots_allows(&disallow, url.path())
        {
            continue;
        }
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }

        // A PRIMEIRA página falhando merece o erro real na tela (403 de
        // anti-bot, DNS errado…); depois que algo já entrou, 404 segue o baile.
        let resp = match client.get(url.clone()).send() {
            Ok(r) if r.status().is_success() => r,
            Ok(r) if files == 0 => {
                let hint = if r.status() == 403 || r.status() == 429 {
                    " — o site provavelmente bloqueia robôs; tente o zimit"
                } else {
                    ""
                };
                return Err(format!("O site respondeu {} na página inicial{}", r.status(), hint));
            }
            Err(e) if files == 0 => return Err(format!("Falha de rede na página inicial: {e}")),
            _ => continue,
        };
        let mut final_url = resp.url().clone();
        final_url.set_fragment(None);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        if resp.content_length().unwrap_or(0) > MAX_FILE {
            continue;
        }
        let mut body = Vec::new();
        if resp.take(MAX_FILE).read_to_end(&mut body).is_err() {
            continue;
        }

        let is_html = ct.contains("text/html");
        // página redirecionada pra outro host não entra
        if is_html && final_url.host_str().map(|h| h.to_lowercase()) != Some(main_host.clone()) {
            continue;
        }
        let is_css = ct.contains("text/css")
            || final_url.path().to_lowercase().ends_with(".css");

        let mut local = url_to_path(&final_url, is_html, &main_host);
        if map.contains_key(final_url.as_str()) {
            continue; // redirect pra algo já baixado
        }
        if used_paths.contains(&local) {
            let h = format!("_{}", short_hash(final_url.as_str()));
            match local.rfind('.') {
                Some(dot) => local.insert_str(dot, &h),
                None => local.push_str(&h),
            }
        }
        used_paths.insert(local.clone());
        let dest = staging.join(local.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&dest, &body).map_err(|e| e.to_string())?;
        files += 1;
        map.insert(final_url.as_str().to_string(), local.clone());
        map.insert(url.as_str().to_string(), local.clone());

        if is_html {
            pages += 1;
            if main_local.is_none() {
                main_local = Some(local.clone());
            }
            let depth = match kind {
                Kind::Page(d) => d,
                Kind::Asset => 0,
            };
            let (links, assets) = extract_links(&String::from_utf8_lossy(&body), &final_url);
            if depth < max_depth {
                for l in links {
                    let same_host = l.host_str().map(|h| h.to_lowercase()) == Some(main_host.clone());
                    if same_host && seen.insert(l.as_str().to_string()) {
                        queue.push_back((l, Kind::Page(depth + 1)));
                    }
                }
            }
            for a in assets {
                if seen.insert(a.as_str().to_string()) {
                    queue.push_back((a, Kind::Asset));
                }
            }
            htmls.push((local, final_url.clone()));
        } else if is_css {
            let css_text = String::from_utf8_lossy(&body).into_owned();
            for r in css_refs(&css_text, &final_url) {
                if seen.insert(r.as_str().to_string()) {
                    queue.push_back((r, Kind::Asset));
                }
            }
            csses.push((local, final_url.clone()));
        }

        on_progress(CrawlProgress {
            pages,
            files,
            queued: queue.len() as u32,
            current: final_url.to_string(),
        });
    }

    if pages == 0 {
        return Err("Não consegui baixar nenhuma página HTML dessa URL".into());
    }

    // Fase 2: reescrita com o conjunto completo de arquivos conhecido
    for (local, base) in &htmls {
        let p = staging.join(local.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Ok(txt) = fs::read_to_string(&p) {
            let _ = fs::write(&p, rewrite_html(&txt, base, local, &map));
        }
    }
    for (local, base) in &csses {
        let p = staging.join(local.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Ok(txt) = fs::read_to_string(&p) {
            let _ = fs::write(&p, rewrite_css(&txt, base, local, &map));
        }
    }

    Ok(CrawlOutcome {
        main_local: main_local.unwrap(),
        pages,
        files,
    })
}

/// URLs referenciadas por um CSS (url() e @import).
fn css_refs(css: &str, base: &Url) -> Vec<Url> {
    let mut out = Vec::new();
    let lower = css.to_ascii_lowercase();
    let mut i = 0;
    while let Some(p) = lower[i..].find("url(") {
        let s = i + p + 4;
        let e = match css[s..].find(')') {
            Some(e) => s + e,
            None => break,
        };
        let raw = css[s..e].trim().trim_matches(['"', '\'']);
        if let Some(u) = resolve(base, raw) {
            out.push(u);
        }
        i = e;
    }
    let mut i = 0;
    while let Some(p) = lower[i..].find("@import ") {
        let s = i + p + 8;
        let rest = &css[s..];
        if let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
            let inner = &rest[1..];
            if let Some(e) = inner.find(q) {
                if let Some(u) = resolve(base, &inner[..e]) {
                    out.push(u);
                }
            }
        }
        i = s;
    }
    out
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn mapeia_urls_pra_caminhos_locais() {
        let h = "site.com";
        assert_eq!(url_to_path(&u("https://site.com/"), true, h), "index.html");
        assert_eq!(url_to_path(&u("https://site.com/guia/"), true, h), "guia/index.html");
        assert_eq!(url_to_path(&u("https://site.com/guia/api"), true, h), "guia/api.html");
        assert_eq!(url_to_path(&u("https://site.com/a.css"), false, h), "a.css");
        // query vira sufixo com hash estável
        let p1 = url_to_path(&u("https://site.com/s.css?v=1"), false, h);
        let p2 = url_to_path(&u("https://site.com/s.css?v=2"), false, h);
        assert!(p1.starts_with("s_") && p1.ends_with(".css") && p1 != p2);
        // host de fora vai pra _ext
        assert_eq!(
            url_to_path(&u("https://cdn.x.io/f.woff2"), false, h),
            "_ext/cdn.x.io/f.woff2"
        );
    }

    #[test]
    fn caminhos_relativos_entre_arquivos() {
        assert_eq!(rel_path("index.html", "css/a.css"), "css/a.css");
        assert_eq!(rel_path("guia/x.html", "css/a.css"), "../css/a.css");
        assert_eq!(rel_path("a/b/c.html", "a/d.png"), "../d.png");
        assert_eq!(rel_path("a/b.html", "a/c.html"), "c.html");
    }

    #[test]
    fn robots_e_reescrita_de_html() {
        let dis = parse_robots("User-agent: *\nDisallow: /admin\nDisallow:\n\nUser-agent: outro\nDisallow: /");
        assert_eq!(dis, vec!["/admin".to_string()]);
        assert!(robots_allows(&dis, "/blog"));
        assert!(!robots_allows(&dis, "/admin/x"));

        let base = u("https://site.com/blog/post.html");
        let mut map = HashMap::new();
        map.insert("https://site.com/blog/outro.html".into(), "blog/outro.html".into());
        map.insert("https://site.com/css/a.css".into(), "css/a.css".into());
        // chave do mapa é sempre a forma percent-encoded (Url::as_str)
        map.insert("https://site.com/img/foto%20grande.png".into(), "img/foto grande.png".into());
        let html = r#"<html><head><base href="/x/"><link rel="stylesheet" href="/css/a.css"></head>
            <body><a href="outro.html#sec">i</a> <a href="https://fora.com/p">e</a>
            <img src="/img/foto%20grande.png"></body></html>"#;
        let out = rewrite_html(html, &base, "blog/post.html", &map);
        assert!(out.contains("href=\"outro.html#sec\""), "{out}");
        assert!(out.contains("href=\"../css/a.css\""));
        assert!(out.contains("src=\"../img/foto%20grande.png\""));
        assert!(out.contains("https://fora.com/p")); // externo fica absoluto
        assert!(!out.contains("<base"), "base tem que sumir");
    }

    #[test]
    fn reescreve_css() {
        let base = u("https://site.com/css/main.css");
        let mut map = HashMap::new();
        map.insert("https://site.com/img/bg.png".into(), "img/bg.png".into());
        map.insert("https://site.com/css/extra.css".into(), "css/extra.css".into());
        let css = "@import \"extra.css\";\nbody{background:url('/img/bg.png') url(https://fora.com/x.png)}";
        let out = rewrite_css(css, &base, "css/main.css", &map);
        assert!(out.contains("@import \"extra.css\""), "{out}");
        assert!(out.contains("url('../img/bg.png')") || out.contains("url(../img/bg.png)"), "{out}");
        assert!(out.contains("https://fora.com/x.png"));
    }

    /// Servidor HTTP mínimo pro teste de integração (sem dependências).
    fn serve_site() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 2048];
                let n = std::io::Read::read(&mut s, &mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (ct, body): (&str, Vec<u8>) = match path.as_str() {
                    "/" => ("text/html", b"<html><head><title>Home</title><link rel=stylesheet href=/estilo.css></head><body><a href=/pagina2>dois</a> <a href=/secreto/x>nao</a> <a href=https://exemplo.com/fora>fora</a><img src=/logo.png></body></html>".to_vec()),
                    "/pagina2" => ("text/html", b"<html><head><title>Dois</title></head><body><a href=/>volta</a></body></html>".to_vec()),
                    "/estilo.css" => ("text/css", b"body{background:url('/fundo.png')}".to_vec()),
                    "/logo.png" => ("image/png", b"\x89PNG-logo".to_vec()),
                    "/fundo.png" => ("image/png", b"\x89PNG-fundo".to_vec()),
                    "/robots.txt" => ("text/plain", b"User-agent: *\nDisallow: /secreto".to_vec()),
                    _ => ("text/html", b"<html><body>404</body></html>".to_vec()),
                };
                let status = if path.starts_with("/secreto") { "404 Not Found" } else { "200 OK" };
                let _ = write!(
                    s,
                    "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status, ct, body.len()
                );
                let _ = s.write_all(&body);
            }
        });
        port
    }

    #[test]
    fn crawl_de_ponta_a_ponta_com_servidor_local() {
        let port = serve_site();
        let staging = std::env::temp_dir().join(format!("localzim-crawl-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);

        let spec = CrawlSpec {
            start_url: format!("http://127.0.0.1:{port}/"),
            max_depth: 3,
            max_pages: 50,
            delay_ms: 0,
        };
        let cancel = AtomicBool::new(false);
        let out = crawl(&spec, &staging, &cancel, |_| {}).unwrap();
        assert_eq!(out.pages, 2);
        assert_eq!(out.main_local, "index.html");

        let index = fs::read_to_string(staging.join("index.html")).unwrap();
        assert!(index.contains("href=\"pagina2.html\""), "{index}");
        assert!(index.contains("src=\"logo.png\""));
        assert!(index.contains("https://exemplo.com/fora")); // externo intacto
        // robots.txt respeitado
        assert!(!staging.join("secreto").exists());
        // CSS reescrito e fundo baixado via url()
        let css = fs::read_to_string(staging.join("estilo.css")).unwrap();
        // o rewriter solta url() sem aspas (CSS válido)
        assert!(css.contains("url(fundo.png)"), "{css}");
        assert!(staging.join("fundo.png").exists());

        let _ = fs::remove_dir_all(&staging);
    }
}
