//! Tradução offline de artigos (pt-BR · es · en) com modelos Marian/OPUS-MT
//! da Helsinki-NLP rodando no candle (CPU, Rust puro — sem runtime externo).
//!
//! Os modelos são convertidos (f16 + tokenizer.json) e publicados como assets
//! da release `v1` do repo Anon5T4R/LocalZIM-models; o app baixa cada direção
//! sob demanda para `app_data/translate/models/<direção>/`, com sha256
//! verificado durante o download. pt↔es não tem modelo dedicado da Helsinki:
//! o app pivota pelo inglês (duas pernas, dois modelos).
//!
//! Traduções ficam em cache por (UUID do arquivo, artigo, direção) — revisitar
//! uma página traduzida é instantâneo e não roda o modelo de novo.

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::marian;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokenizers::Tokenizer;

/// Manifesto gerado na conversão dos modelos (sha256/tamanho por arquivo).
const MANIFEST: &str = include_str!("translate_manifest.json");
/// Release do GitHub que hospeda os bundles convertidos.
const BASE_URL: &str = "https://github.com/Anon5T4R/LocalZIM-models/releases/download/v1";
/// Modelo sai da RAM depois deste tempo sem uso.
const IDLE_EVICT: Duration = Duration::from_secs(5 * 60);
/// Limite de tokens de entrada por chunk (Marian é treinado até 512 posições).
const MAX_SRC_TOKENS: usize = 480;
/// Alvo de tamanho de chunk (caracteres) ao agrupar sentenças curtas.
const CHUNK_CHARS: usize = 400;

// ---------- manifesto ----------

#[derive(Deserialize)]
struct Manifest {
    directions: HashMap<String, DirectionSpec>,
}

#[derive(Deserialize)]
struct DirectionSpec {
    /// Token de idioma alvo exigido pelo modelo (ex.: ">>pob<<"), ou vazio.
    #[serde(default)]
    prefix: String,
    files: HashMap<String, FileSpec>,
}

#[derive(Deserialize)]
struct FileSpec {
    sha256: String,
    bytes: u64,
}

fn manifest() -> &'static Manifest {
    static M: std::sync::OnceLock<Manifest> = std::sync::OnceLock::new();
    M.get_or_init(|| serde_json::from_str(MANIFEST).expect("translate_manifest.json inválido"))
}

/// Direções servidas por um único modelo. pt↔es pivota pelo inglês.
fn legs(direction: &str) -> Option<Vec<&'static str>> {
    match direction {
        "en-pt" => Some(vec!["en-pt"]),
        "pt-en" => Some(vec!["pt-en"]),
        "en-es" => Some(vec!["en-es"]),
        "es-en" => Some(vec!["es-en"]),
        "pt-es" => Some(vec!["pt-en", "en-es"]),
        "es-pt" => Some(vec!["es-en", "en-pt"]),
        _ => None,
    }
}

pub fn models_dir(app_data: &Path) -> PathBuf {
    app_data.join("translate").join("models")
}

fn leg_dir(app_data: &Path, leg: &str) -> PathBuf {
    models_dir(app_data).join(leg)
}

/// Um modelo está instalado se todos os arquivos existem com o tamanho certo
/// (o sha256 é conferido uma única vez, durante o download).
pub fn leg_installed(app_data: &Path, leg: &str) -> bool {
    let Some(spec) = manifest().directions.get(leg) else {
        return false;
    };
    let dir = leg_dir(app_data, leg);
    spec.files.iter().all(|(name, f)| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.len() == f.bytes)
            .unwrap_or(false)
    })
}

fn leg_bytes(leg: &str) -> u64 {
    manifest()
        .directions
        .get(leg)
        .map(|d| d.files.values().map(|f| f.bytes).sum())
        .unwrap_or(0)
}

// ---------- estado ----------

pub struct Translator {
    /// Modelos carregados na RAM, por perna, com o instante do último uso.
    engines: Mutex<HashMap<String, Arc<Mutex<Engine>>>>,
    last_use: Mutex<HashMap<String, Instant>>,
    /// Downloads em andamento (flag de cancelamento), por perna.
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Default for Translator {
    fn default() -> Self {
        Self {
            engines: Mutex::new(HashMap::new()),
            last_use: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
        }
    }
}

impl Translator {
    /// Descarta modelos sem uso há mais de `IDLE_EVICT` (chamado por um timer).
    pub fn evict_idle(&self) {
        let now = Instant::now();
        let stale: Vec<String> = self
            .last_use
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, t)| now.duration_since(**t) > IDLE_EVICT)
            .map(|(k, _)| k.clone())
            .collect();
        if stale.is_empty() {
            return;
        }
        let mut engines = self.engines.lock().unwrap();
        let mut last = self.last_use.lock().unwrap();
        for k in stale {
            engines.remove(&k);
            last.remove(&k);
        }
    }

    pub fn downloading(&self) -> HashMap<String, ()> {
        self.downloads
            .lock()
            .unwrap()
            .keys()
            .map(|k| (k.clone(), ()))
            .collect()
    }
}

// ---------- engine (um modelo Marian carregado) ----------

struct Engine {
    model: marian::MTModel,
    config: marian::Config,
    tok_src: Tokenizer,
    tok_tgt: Tokenizer,
    prefix: String,
}

/// config.json dos OPUS-MT antigos não tem todas as chaves que o
/// marian::Config exige — preenche defaults antes de desserializar.
fn load_marian_config(path: &Path) -> Result<marian::Config, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let o = v.as_object_mut().ok_or("config.json não é um objeto")?;
    let eos = o.get("eos_token_id").cloned().unwrap_or(0.into());
    o.entry("share_encoder_decoder_embeddings").or_insert(true.into());
    o.entry("use_cache").or_insert(true.into());
    o.entry("is_encoder_decoder").or_insert(true.into());
    o.entry("forced_eos_token_id").or_insert(eos);
    serde_json::from_value(v).map_err(|e| format!("config.json incompatível: {e}"))
}

impl Engine {
    fn load(app_data: &Path, leg: &str) -> Result<Self, String> {
        let spec = manifest()
            .directions
            .get(leg)
            .ok_or_else(|| format!("direção desconhecida: {leg}"))?;
        let dir = leg_dir(app_data, leg);
        let config = load_marian_config(&dir.join("config.json"))?;
        let tok_src = Tokenizer::from_file(dir.join("tokenizer-src.json"))
            .map_err(|e| format!("tokenizer de origem: {e}"))?;
        let tok_tgt = Tokenizer::from_file(dir.join("tokenizer-tgt.json"))
            .map_err(|e| format!("tokenizer de destino: {e}"))?;
        // Pesos f16 no disco viram f32 na RAM (matmul f32 é o caminho rápido na CPU).
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                DType::F32,
                &Device::Cpu,
            )
            .map_err(|e| format!("pesos do modelo: {e}"))?
        };
        let model = marian::MTModel::new(&config, vb).map_err(|e| format!("modelo: {e}"))?;
        Ok(Self {
            model,
            config,
            tok_src,
            tok_tgt,
            prefix: spec.prefix.clone(),
        })
    }

    /// Traduz um chunk (uma ou poucas sentenças) com decodificação gulosa.
    fn translate_chunk(&mut self, text: &str) -> Result<String, String> {
        let text = if self.prefix.is_empty() {
            text.to_string()
        } else {
            format!("{} {}", self.prefix, text)
        };
        let mut tokens = self
            .tok_src
            .encode(text.as_str(), true)
            .map_err(|e| e.to_string())?
            .get_ids()
            .to_vec();
        tokens.truncate(MAX_SRC_TOKENS);
        tokens.push(self.config.eos_token_id);

        let device = Device::Cpu;
        let input = Tensor::new(tokens.as_slice(), &device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| e.to_string())?;
        self.model.reset_kv_cache();
        let encoder_xs = self
            .model
            .encoder()
            .forward(&input, 0)
            .map_err(|e| e.to_string())?;

        // Sem temperatura = argmax (tradução quer a saída mais provável).
        let mut lp = LogitsProcessor::new(0, None, None);
        let mut out_ids = vec![self.config.decoder_start_token_id];
        for index in 0..512 {
            let context_size = if index >= 1 { 1 } else { out_ids.len() };
            let start_pos = out_ids.len().saturating_sub(context_size);
            let step = (|| -> candle_core::Result<u32> {
                let input_ids = Tensor::new(&out_ids[start_pos..], &device)?.unsqueeze(0)?;
                let logits = self.model.decode(&input_ids, &encoder_xs, start_pos)?;
                let logits = logits.squeeze(0)?;
                let logits = logits.get(logits.dim(0)? - 1)?;
                lp.sample(&logits)
            })();
            let token = step.map_err(|e| e.to_string())?;
            if token == self.config.eos_token_id || token == self.config.forced_eos_token_id {
                break;
            }
            out_ids.push(token);
        }
        let decoded = self
            .tok_tgt
            .decode(&out_ids[1..], true)
            .map_err(|e| e.to_string())?;
        Ok(clean_output(&decoded, &self.prefix))
    }

    fn translate_text(&mut self, text: &str) -> Result<String, String> {
        let mut out: Vec<String> = Vec::new();
        for chunk in split_chunks(text, CHUNK_CHARS) {
            out.push(self.translate_chunk(&chunk)?);
        }
        Ok(out.join(" "))
    }
}

// ---------- divisão em sentenças/chunks ----------

/// Divide o texto em chunks de até ~`budget` caracteres, quebrando em
/// fronteiras de sentença (. ! ? … seguidos de espaço). Sentenças curtas são
/// agrupadas — Marian traduz melhor (e mais rápido) com contexto de sentença.
pub fn split_chunks(text: &str, budget: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…') {
            // fim de sentença só se vier espaço depois (evita "3.14", "e.g.x")
            if chars.peek().map(|n| n.is_whitespace()).unwrap_or(true) {
                // abreviações de uma letra ("D. Pedro") e números ordinais seguem juntos
                let tail: String = cur
                    .trim_end_matches(|ch: char| matches!(ch, '.' | '!' | '?' | '…'))
                    .chars()
                    .rev()
                    .take_while(|ch| !ch.is_whitespace())
                    .collect();
                if !(c == '.' && tail.chars().count() <= 1) {
                    while chars.peek().map(|n| n.is_whitespace()).unwrap_or(false) {
                        chars.next();
                    }
                    sentences.push(std::mem::take(&mut cur).trim().to_string());
                }
            }
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur.trim().to_string());
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut acc = String::new();
    for s in sentences {
        if !acc.is_empty() && acc.chars().count() + s.chars().count() + 1 > budget {
            chunks.push(std::mem::take(&mut acc));
        }
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(&s);
    }
    if !acc.is_empty() {
        chunks.push(acc);
    }
    chunks
}

/// Modelos com token de idioma alvo (">>pob<<") ecoam esse token no começo
/// da saída, e os marcadores ">>"/"<<" não existem no vocabulário de destino
/// (viram "<NIL>"). Remove o eco e qualquer "<NIL>" residual.
fn clean_output(decoded: &str, prefix: &str) -> String {
    let mut s = decoded.replace("<NIL>", "");
    if !prefix.is_empty() {
        let lang_id = prefix.trim_matches(|c| c == '>' || c == '<');
        let t = s.trim_start();
        if let Some(rest) = t.strip_prefix(lang_id) {
            if rest.starts_with(char::is_whitespace) {
                s = rest.trim_start().to_string();
            }
        }
    }
    s.trim().to_string()
}

/// Textos sem letra nenhuma (números, símbolos, citações "[1]") não passam
/// pelo modelo — voltam como estão.
fn needs_translation(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic())
}

// ---------- cache ----------

fn cache_file(app_data: &Path, uuid: &str, direction: &str, article: &str) -> PathBuf {
    let mut h = Sha256::new();
    h.update(article.as_bytes());
    let name = format!("{}-{:x}.json", direction, h.finalize());
    app_data
        .join("translate")
        .join("cache")
        .join(uuid)
        .join(name)
}

fn text_key(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())[..16].to_string()
}

fn cache_load(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn cache_save(path: &Path, map: &HashMap<String, String>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(map) {
        let _ = std::fs::write(path, s);
    }
}

// ---------- download ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirectionStatus {
    pub direction: String,
    /// Pernas (modelos) necessárias; 2 quando pivota pelo inglês.
    pub legs: Vec<LegStatus>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LegStatus {
    pub leg: String,
    pub installed: bool,
    pub downloading: bool,
    pub bytes: u64,
}

pub fn direction_status(app_data: &Path, tr: &Translator, direction: &str) -> Option<DirectionStatus> {
    let downloading = tr.downloading();
    let legs = legs(direction)?
        .into_iter()
        .map(|leg| LegStatus {
            leg: leg.to_string(),
            installed: leg_installed(app_data, leg),
            downloading: downloading.contains_key(leg),
            bytes: leg_bytes(leg),
        })
        .collect();
    Some(DirectionStatus {
        direction: direction.to_string(),
        legs,
    })
}

/// Baixa os arquivos de uma perna, com sha256 conferido em streaming e
/// progresso via callback (bytes recebidos / total).
pub fn download_leg(
    app_data: &Path,
    leg: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let spec = manifest()
        .directions
        .get(leg)
        .ok_or_else(|| format!("direção desconhecida: {leg}"))?;
    let dir = leg_dir(app_data, leg);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let total: u64 = spec.files.values().map(|f| f.bytes).sum();
    let mut received: u64 = 0;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    // ordena pra baixar os arquivos pequenos primeiro (feedback imediato)
    let mut names: Vec<&String> = spec.files.keys().collect();
    names.sort_by_key(|n| spec.files[*n].bytes);

    for name in names {
        let fspec = &spec.files[name];
        let dest = dir.join(name);
        if std::fs::metadata(&dest).map(|m| m.len() == fspec.bytes).unwrap_or(false) {
            received += fspec.bytes;
            progress(received, total);
            continue;
        }
        let url = format!("{BASE_URL}/{leg}-{name}");
        let mut resp = client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("falha baixando {name}: {e}"))?;

        let part = dir.join(format!("{name}.part"));
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(out);
                let _ = std::fs::remove_file(&part);
                return Err("cancelado".into());
            }
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            received += n as u64;
            progress(received, total);
        }
        drop(out);
        let got = format!("{:x}", hasher.finalize());
        if got != fspec.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{name}: checksum não confere (download corrompido?)"));
        }
        std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn start_download(tr: &Translator, leg: &str) -> Option<Arc<AtomicBool>> {
    let mut dl = tr.downloads.lock().unwrap();
    if dl.contains_key(leg) {
        return None;
    }
    let flag = Arc::new(AtomicBool::new(false));
    dl.insert(leg.to_string(), flag.clone());
    Some(flag)
}

pub fn finish_download(tr: &Translator, leg: &str) {
    tr.downloads.lock().unwrap().remove(leg);
}

pub fn cancel_download(tr: &Translator, leg: &str) {
    if let Some(f) = tr.downloads.lock().unwrap().get(leg) {
        f.store(true, Ordering::Relaxed);
    }
}

pub fn remove_leg(app_data: &Path, tr: &Translator, leg: &str) -> Result<(), String> {
    tr.engines.lock().unwrap().remove(leg);
    tr.last_use.lock().unwrap().remove(leg);
    let dir = leg_dir(app_data, leg);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- tradução ----------

fn engine_for(
    app_data: &Path,
    tr: &Translator,
    leg: &str,
) -> Result<Arc<Mutex<Engine>>, String> {
    if let Some(e) = tr.engines.lock().unwrap().get(leg) {
        tr.last_use
            .lock()
            .unwrap()
            .insert(leg.to_string(), Instant::now());
        return Ok(e.clone());
    }
    if !leg_installed(app_data, leg) {
        return Err(format!("o modelo {leg} não está instalado"));
    }
    let engine = Arc::new(Mutex::new(Engine::load(app_data, leg)?));
    tr.engines
        .lock()
        .unwrap()
        .insert(leg.to_string(), engine.clone());
    tr.last_use
        .lock()
        .unwrap()
        .insert(leg.to_string(), Instant::now());
    Ok(engine)
}

/// Carrega na RAM os modelos de uma direção (chamado antes do 1º lote pra UI
/// poder mostrar "carregando modelo" separado de "traduzindo").
pub fn prepare(app_data: &Path, tr: &Translator, direction: &str) -> Result<(), String> {
    for leg in legs(direction).ok_or("direção inválida")? {
        engine_for(app_data, tr, leg)?;
    }
    Ok(())
}

/// Traduz um lote de blocos de texto. Consulta/alimenta o cache do artigo;
/// só o que não está em cache passa pelo modelo (pivotando quando preciso).
pub fn translate_texts(
    app_data: &Path,
    tr: &Translator,
    uuid: &str,
    article: &str,
    direction: &str,
    texts: Vec<String>,
) -> Result<Vec<String>, String> {
    let legs = legs(direction).ok_or("direção inválida")?;
    let cpath = cache_file(app_data, uuid, direction, article);
    let mut cache = cache_load(&cpath);

    let mut out: Vec<Option<String>> = texts
        .iter()
        .map(|t| {
            if !needs_translation(t) {
                Some(t.clone())
            } else {
                cache.get(&text_key(t)).cloned()
            }
        })
        .collect();

    let pending: Vec<usize> = (0..texts.len()).filter(|&i| out[i].is_none()).collect();
    if !pending.is_empty() {
        // pernas em sequência: pt→es vira pt→en→es
        let mut work: Vec<String> = pending.iter().map(|&i| texts[i].clone()).collect();
        for leg in &legs {
            let engine = engine_for(app_data, tr, leg)?;
            let mut eng = engine.lock().unwrap();
            for t in work.iter_mut() {
                *t = eng.translate_text(t)?;
            }
            tr.last_use
                .lock()
                .unwrap()
                .insert(leg.to_string(), Instant::now());
        }
        let mut changed = false;
        for (slot, translated) in pending.iter().zip(work) {
            cache.insert(text_key(&texts[*slot]), translated.clone());
            out[*slot] = Some(translated);
            changed = true;
        }
        if changed {
            cache_save(&cpath, &cache);
        }
    }

    Ok(out.into_iter().map(|o| o.unwrap_or_default()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_agrupa_sentencas_curtas_num_chunk() {
        let out = split_chunks("Olá mundo. Tudo bem? Sim!", 400);
        assert_eq!(out, vec!["Olá mundo. Tudo bem? Sim!"]);
    }

    #[test]
    fn split_quebra_no_orcamento_em_fronteira_de_sentenca() {
        let a = "a".repeat(300);
        let b = "b".repeat(300);
        let text = format!("{a}. {b}.");
        let out = split_chunks(&text, 400);
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with('a') && out[0].ends_with('.'));
        assert!(out[1].starts_with('b'));
    }

    #[test]
    fn split_nao_quebra_em_numeros_nem_abreviacao_de_uma_letra() {
        let out = split_chunks("O valor de pi é 3.14 aproximadamente. D. Pedro reinou.", 40);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("3.14"));
        assert_eq!(out[1], "D. Pedro reinou.");
    }

    #[test]
    fn split_texto_sem_pontuacao_vira_um_chunk() {
        let out = split_chunks("título de seção sem ponto final", 400);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn textos_sem_letras_nao_precisam_de_modelo() {
        assert!(!needs_translation("[1]"));
        assert!(!needs_translation("3.14 × 10²"));
        assert!(needs_translation("Olá"));
        assert!(needs_translation("½ xícara"));
    }

    #[test]
    fn limpeza_remove_eco_do_token_de_idioma() {
        assert_eq!(
            clean_output("<NIL>pob<NIL> Rust é uma linguagem.", ">>pob<<"),
            "Rust é uma linguagem."
        );
        // "pob" no meio do texto não é tocado; sem prefixo, só tira <NIL>
        assert_eq!(clean_output("Sigla pob aqui.", ""), "Sigla pob aqui.");
        assert_eq!(clean_output("Plain english.", ""), "Plain english.");
    }

    #[test]
    fn pivo_pt_es_usa_duas_pernas() {
        assert_eq!(legs("pt-es").unwrap(), vec!["pt-en", "en-es"]);
        assert_eq!(legs("es-pt").unwrap(), vec!["es-en", "en-pt"]);
        assert_eq!(legs("en-pt").unwrap(), vec!["en-pt"]);
        assert!(legs("en-fr").is_none());
    }

    /// De ponta a ponta com modelo real (não roda no CI): aponte
    /// LOCALZIM_TEST_APPDATA pra uma pasta com translate/models/en-es/
    /// preenchida com um bundle convertido.
    #[test]
    fn traducao_de_ponta_a_ponta_com_bundle_local() {
        let Ok(app_data) = std::env::var("LOCALZIM_TEST_APPDATA") else {
            eprintln!("LOCALZIM_TEST_APPDATA não definido — teste pulado");
            return;
        };
        let app_data = PathBuf::from(app_data);
        assert!(leg_installed(&app_data, "en-es"), "bundle en-es ausente/incompleto");
        let tr = Translator::default();
        let texts = vec![
            "The library contains the entire Wikipedia.".to_string(),
            "[1]".to_string(), // sem letras: não passa pelo modelo
        ];
        let out = translate_texts(&app_data, &tr, "cafe1234", "A/Teste", "en-es", texts.clone())
            .expect("tradução falhou");
        assert_eq!(out.len(), 2);
        assert!(out[0].to_lowercase().contains("wikipedia"), "saída: {}", out[0]);
        assert_ne!(out[0], texts[0]);
        assert_eq!(out[1], "[1]");
        // segunda chamada sai do cache (e não do modelo)
        let again = translate_texts(&app_data, &tr, "cafe1234", "A/Teste", "en-es", texts)
            .expect("cache falhou");
        assert_eq!(again[0], out[0]);
        let cache_dir = app_data.join("translate").join("cache").join("cafe1234");
        assert!(cache_dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false));
    }

    #[test]
    fn manifesto_embutido_e_valido_e_cobre_as_4_pernas() {
        let m = manifest();
        for leg in ["en-pt", "pt-en", "en-es", "es-en"] {
            let d = m.directions.get(leg).expect(leg);
            for f in ["model.safetensors", "config.json", "tokenizer-src.json", "tokenizer-tgt.json"] {
                assert!(d.files.contains_key(f), "{leg} sem {f}");
            }
        }
        assert_eq!(m.directions["en-pt"].prefix, ">>pob<<");
    }
}
