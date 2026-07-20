//! Busca full-text local por arquivo ZIM.
//!
//! O índice Xapian embutido nos ZIM só é legível via libzim/xapian (C++);
//! em vez disso o LocalZIM constrói o próprio índice **tantivy** uma única
//! vez, em segundo plano, e o guarda na pasta de dados do app, chaveado pelo
//! UUID do arquivo. Tokenização minúscula + sem acentos (busca "sao paulo"
//! acha "São Paulo"); o corpo não fica armazenado no índice (economia de
//! disco) — o trecho destacado é re-extraído do ZIM na hora da busca.

use crate::zim::{Kind, ZimFile};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexWriter, TantivyDocument};

const TOKENIZER: &str = "folding";
const WRITER_HEAP: usize = 192 * 1024 * 1024;
pub const DONE_FILE: &str = "localzim-done.json";

/// Estado compartilhado de uma indexação em andamento.
pub struct FtBuild {
    pub cancel: AtomicBool,
    /// 0..=1000 (permilagem do arquivo varrido).
    pub progress: AtomicU32,
}

impl FtBuild {
    pub fn new() -> FtBuild {
        FtBuild {
            cancel: AtomicBool::new(false),
            progress: AtomicU32::new(0),
        }
    }
}

pub struct FtIndex {
    index: Index,
    path_f: Field,
    title_f: Field,
    body_f: Field,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FtHit {
    pub title: String,
    pub path: String,
    /// HTML escapado pelo tantivy, com os termos em `<b>`.
    pub snippet: String,
    pub score: f32,
}

fn folding_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .build()
}

fn build_schema() -> (Schema, Field, Field, Field) {
    let mut sb = Schema::builder();
    let text = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let path_f = sb.add_text_field("path", STORED);
    let title_f = sb.add_text_field("title", text.clone().set_stored());
    let body_f = sb.add_text_field("body", text);
    (sb.build(), path_f, title_f, body_f)
}

/// Total de documentos se o índice em `dir` está completo.
pub fn is_ready(dir: &Path) -> Option<u64> {
    let meta = fs::read_to_string(dir.join(DONE_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&meta).ok()?;
    v.get("docs").and_then(|d| d.as_u64())
}

pub fn open(dir: &Path) -> tantivy::Result<FtIndex> {
    let index = Index::open_in_dir(dir)?;
    index.tokenizers().register(TOKENIZER, folding_analyzer());
    let schema = index.schema();
    Ok(FtIndex {
        path_f: schema.get_field("path")?,
        title_f: schema.get_field("title")?,
        body_f: schema.get_field("body")?,
        index,
    })
}

/// Varre o ZIM inteiro e indexa os artigos HTML. Devolve o total indexado.
pub fn build(
    zim: &ZimFile,
    dir: &Path,
    st: &FtBuild,
    mut on_progress: impl FnMut(f32),
) -> Result<u64, String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (schema, path_f, title_f, body_f) = build_schema();
    let index = Index::create_in_dir(dir, schema).map_err(|e| e.to_string())?;
    index.tokenizers().register(TOKENIZER, folding_analyzer());
    let writer: IndexWriter = index.writer(WRITER_HEAP).map_err(|e| e.to_string())?;

    let total = zim.header.entry_count;
    let ans = zim.article_namespace();
    let mut docs: u64 = 0;
    for i in 0..total {
        if st.cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = fs::remove_dir_all(dir);
            return Err("indexação cancelada".into());
        }
        if let Ok(d) = zim.entry_at(i) {
            if d.namespace == ans && matches!(d.kind, Kind::Content { .. }) && zim.is_html(&d) {
                if let Ok(Some((_, bytes))) = zim.content(&d) {
                    let text = html_to_text(&bytes);
                    if !text.is_empty() {
                        writer
                            .add_document(doc!(
                                path_f => d.entry_path(),
                                title_f => d.title_or_url().to_string(),
                                body_f => text,
                            ))
                            .map_err(|e| e.to_string())?;
                        docs += 1;
                    }
                }
            }
        }
        if i % 512 == 0 {
            let p = i as f32 / total.max(1) as f32;
            st.progress.store((p * 1000.0) as u32, Ordering::Relaxed);
            on_progress(p);
        }
    }
    let mut writer = writer;
    writer.commit().map_err(|e| e.to_string())?;
    drop(writer);
    st.progress.store(1000, Ordering::Relaxed);
    fs::write(dir.join(DONE_FILE), format!("{{\"docs\":{docs}}}")).map_err(|e| e.to_string())?;
    Ok(docs)
}

pub fn search(ft: &FtIndex, zim: &ZimFile, q: &str, limit: usize) -> Result<Vec<FtHit>, String> {
    let reader = ft.index.reader().map_err(|e| e.to_string())?;
    let searcher = reader.searcher();
    let mut parser = QueryParser::for_index(&ft.index, vec![ft.title_f, ft.body_f]);
    parser.set_field_boost(ft.title_f, 2.0);
    let (query, _errs) = parser.parse_query_lenient(q);
    let top = searcher
        .search(&*query, &TopDocs::with_limit(limit.clamp(1, 50)))
        .map_err(|e| e.to_string())?;
    let mut sg =
        SnippetGenerator::create(&searcher, &*query, ft.body_f).map_err(|e| e.to_string())?;
    sg.set_max_num_chars(240);

    let mut out = Vec::with_capacity(top.len());
    for (score, addr) in top {
        let doc: TantivyDocument = searcher.doc(addr).map_err(|e| e.to_string())?;
        let get = |f: Field| {
            doc.get_first(f)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let path = get(ft.path_f);
        let snippet = lookup_text(zim, &path)
            .map(|t| sg.snippet(&t).to_html())
            .unwrap_or_default();
        out.push(FtHit {
            title: get(ft.title_f),
            path,
            snippet,
            score,
        });
    }
    Ok(out)
}

fn lookup_text(zim: &ZimFile, path: &str) -> Option<String> {
    let (ns, url) = path.split_once('/')?;
    let (_, d) = zim.find(*ns.as_bytes().first()?, url).ok()??;
    let (_, bytes) = zim.content(&d).ok()??;
    Some(html_to_text(&bytes))
}

/// Extração de texto bem simples: descarta script/style, tags e entidades comuns.
pub fn html_to_text(html: &[u8]) -> String {
    let s = String::from_utf8_lossy(html);
    // ASCII lowercase preserva os índices de byte do original
    let low = s.to_ascii_lowercase();
    let src = s.as_bytes();
    let n = src.len();
    let mut out: Vec<u8> = Vec::with_capacity(n / 3);
    let mut i = 0usize;
    while i < n {
        if src[i] == b'<' {
            if low[i..].starts_with("<script") || low[i..].starts_with("<style") {
                let close = if low[i..].starts_with("<script") { "</script" } else { "</style" };
                i = low[i..].find(close).map(|p| i + p).unwrap_or(n);
            }
            i = low[i..].find('>').map(|p| i + p + 1).unwrap_or(n);
            out.push(b' ');
        } else {
            out.push(src[i]);
            i += 1;
        }
    }
    let text = String::from_utf8_lossy(&out)
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrai_texto_de_html() {
        let html = b"<html><head><title>T</title><style>p{color:red}</style>\
            <script>var x = '<b>nao</b>';</script></head>\
            <body><h1>Ol&aacute; Ola</h1><p>Texto &amp; mais <b>texto</b>.</p></body></html>";
        let t = html_to_text(html);
        assert!(t.contains("Ola"));
        // cada tag vira um espaço, então pode sobrar espaço antes da pontuação
        assert!(t.contains("Texto & mais texto"));
        assert!(!t.contains("color"));
        assert!(!t.contains("var x"));
    }

    #[test]
    fn indexa_e_busca_num_diretorio_temporario() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("localzim-ft-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (schema, path_f, title_f, body_f) = build_schema();
        let index = Index::create_in_dir(&dir, schema).unwrap();
        index.tokenizers().register(TOKENIZER, folding_analyzer());
        let mut writer: IndexWriter = index.writer(15_000_000).unwrap();
        writer
            .add_document(doc!(
                path_f => "A/Sao_Paulo.html",
                title_f => "São Paulo",
                body_f => "São Paulo é a maior cidade do Brasil.",
            ))
            .unwrap();
        writer.commit().unwrap();
        drop(writer);
        fs::write(dir.join(DONE_FILE), "{\"docs\":1}").unwrap();

        assert_eq!(is_ready(&dir), Some(1));
        let ft = open(&dir).unwrap();
        // busca sem acento acha o título com acento (ascii folding)
        let reader = ft.index.reader().unwrap();
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&ft.index, vec![ft.title_f, ft.body_f]);
        let (q, _) = parser.parse_query_lenient("sao paulo cidade");
        let top = searcher.search(&*q, &TopDocs::with_limit(5)).unwrap();
        assert_eq!(top.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
