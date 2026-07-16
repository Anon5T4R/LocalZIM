import { useSyncExternalStore } from "react";

/**
 * i18n leve da UI (mesmo padrão do LocalCode/LocalTranslate). `pt` é a fonte da
 * verdade das chaves; `en`/`es` como `Record<MessageKey, string>` fazem o
 * compilador recusar chave faltando ou sobrando. Locale num store externo pra
 * `t()` rodar fora de componente. O App remonta na troca (key={locale}).
 *
 * NÃO traduzir: marca (LocalZIM), endônimos de idioma (`LANG_NAMES` em
 * lang.ts), e o idioma do CONTEÚDO do .zim (metadados/`guessLang`). Os nomes
 * dos modelos de tradução vêm de `legName()` (montado a partir dos endônimos).
 */

export type Locale = "pt" | "en" | "es";

export const LOCALE_LABELS: Record<Locale, string> = {
  pt: "Português",
  en: "English",
  es: "Español",
};

const LOCALE_KEY = "localzim.locale";

const pt = {
  // ----- Library -----
  "lib.sub": "Sua biblioteca offline — leitor de arquivos ZIM (Wikipédia, Stack Overflow, Gutenberg…)",
  "lib.openBtn": "Abrir arquivo .zim…",
  "lib.createFolder": "Criar .zim de uma pasta…",
  "lib.createFolderTitle": "Empacota uma pasta com HTML num arquivo .zim",
  "lib.createSite": "Criar .zim de um site…",
  "lib.createSiteTitle": "Baixa um site (crawler local) e empacota num .zim",
  "lib.open": "Abertos",
  "lib.recent": "Recentes",
  "lib.closeFile": "Fechar este arquivo",
  "lib.removeRecent": "Remover dos recentes",
  "lib.articles": "{n} artigos",
  "lib.emptyPre": "Nenhum arquivo aberto ainda. Abra um",
  "lib.emptyMid": "do seu computador — dá para baixar a Wikipédia inteira, Wikcionário, Stack Overflow e muito mais em",
  "lib.defaultLibrary": "Biblioteca",
  "lib.defaultSite": "Site",

  // Diálogos de arquivo
  "dlg.openZim": "Abrir arquivo ZIM",
  "dlg.zimFiles": "Arquivos ZIM",
  "dlg.zimFile": "Arquivo ZIM",
  "dlg.sourceDir": "Pasta com o conteúdo (HTML)",
  "dlg.saveZim": "Salvar arquivo ZIM",

  // Erros de criação
  "err.noSource": "Escolha a pasta de origem.",
  "err.noOutput": 'Escolha onde salvar o arquivo .zim (botão "Escolher…" em Salvar como).',
  "err.noUrl": "Informe o endereço do site.",

  // Modal criar de site
  "site.title": "Criar .zim de um site",
  "site.hintPre":
    "Crawler local: baixa as páginas a partir da URL inicial (respeitando o robots.txt) com imagens, CSS e scripts, reescreve os links e empacota. Acha capítulos listados em JS de navegação (docs mdBook, ex.: o livro do Rust). Funciona bem pra documentação, blogs e wikis; sites montados por JavaScript (SPA) podem sair incompletos — pra esses, use o",
  "site.url": "Endereço do site",
  "site.urlPlaceholder": "https://docs.exemplo.com",
  "site.depth": "Profundidade de links",
  "site.maxPages": "Máximo de páginas",
  "site.samePathPre": "Baixar só o que está dentro do caminho inicial (ex.: começou em",
  "site.samePathPost": ", não sai dele) — recomendado",
  "site.crawling": "Baixando páginas… {pages} (fila: {queued})",
  "site.doneBanner": "Pronto: {articles} páginas, {size} — o arquivo já foi aberto na biblioteca.",
  "site.download": "Baixar e criar",

  // Modal criar de pasta
  "folder.title": "Criar .zim de uma pasta",
  "folder.hintPre":
    "Empacota uma pasta com HTML (site salvo, documentação, notas exportadas) num arquivo",
  "folder.hintMid": "Links relativos entre as páginas continuam funcionando. Para capturar um site da internet, use o",
  "folder.hintPost": "(ou",
  "folder.hintEnd": "e empacote aqui).",
  "folder.source": "Pasta de origem",
  "folder.sourcePlaceholder": "escolha a pasta com o conteúdo",
  "folder.creator": "Criador",
  "folder.mainPage": "Página inicial",
  "folder.mainPlaceholder": "auto (index.html)",
  "folder.packing": "Empacotando… {pct}%",
  "folder.doneBanner": "Pronto: {articles} artigos, {size} — o arquivo já foi aberto na biblioteca.",
  "folder.create": "Criar",

  // Campos comuns dos modais
  "form.saveAs": "Salvar como",
  "form.outputPlaceholder": "destino do arquivo .zim",
  "form.choose": "Escolher…",
  "form.title": "Título",
  "form.language": "Idioma",
  "form.description": "Descrição",
  "common.cancel": "Cancelar",
  "common.close": "Fechar",

  // ----- Reader: toolbar -----
  "rd.library": "Biblioteca",
  "rd.back": "Voltar (Alt+←)",
  "rd.forward": "Avançar (Alt+→)",
  "rd.home": "Página principal",
  "rd.random": "Artigo aleatório",
  "rd.searchPlaceholder": "Buscar em {name}… (Ctrl+K)",
  "rd.zoomOut": "Diminuir zoom (Ctrl+-)",
  "rd.zoomIn": "Aumentar zoom (Ctrl+=)",
  "rd.findTitle": "Localizar na página (Ctrl+F)",
  "rd.translateTitle": "Traduzir página (offline)",
  "rd.themeTitle": "Alternar tema claro/escuro",
  "rd.searchFt": 'Buscar “{q}” no texto completo',
  "rd.iframeTitle": "Conteúdo do arquivo ZIM",

  // Localizar na página
  "find.placeholder": "Localizar na página…",
  "find.prev": "Anterior (Shift+Enter)",
  "find.next": "Próximo (Enter)",
  "find.close": "Fechar (Esc)",

  // Aviso de link externo
  "ext.title": "Sair do arquivo",
  "ext.hintPre": "Este link não faz parte do arquivo",
  "ext.hintMid": "— ele aponta para",
  "ext.hintPost": ", na internet. Quer abrir no seu navegador?",
  "ext.dontWarn": "Não avisar de novo enquanto o LocalZIM estiver aberto",
  "ext.openBrowser": "Abrir no navegador",

  // Tradução offline
  "tr.title": "Tradução offline",
  "tr.translateTo": "Traduzir para",
  "tr.articleLang": "Idioma do artigo",
  "tr.detected": "Detectado: {name}",
  "tr.autoDetect": "Detectar automaticamente",
  "tr.noDetect":
    "Não deu pra detectar o idioma deste artigo — escolha acima. A tradução funciona entre português, espanhol e inglês.",
  "tr.already": "O artigo já está em {name}.",
  "tr.firstTimePre": "Primeira vez nesta direção: o LocalZIM baixa o modelo de tradução",
  "tr.firstTimeStrong": "uma única vez",
  "tr.firstTimePost": "e depois funciona 100% offline.",
  "tr.viaEnglish": " Português ↔ espanhol passa pelo inglês, então são dois modelos.",
  "tr.preparing": "preparando…",
  "tr.downloadModels": "Baixar modelos ({size})",
  "tr.downloadModel": "Baixar modelo ({size})",
  "tr.translatePage": "Traduzir página",
  "tr.loadingModel": "Carregando modelo…",
  "tr.translating": "Traduzindo… {done}/{total} blocos",
  "tr.stop": "Parar",
  "tr.doneCached": "✓ Página traduzida (fica em cache — voltar aqui é instantâneo).",
  "tr.showTranslation": "Ver tradução",
  "tr.showOriginal": "Ver original",
  "tr.autoNext": "Traduzir as próximas páginas automaticamente",
  "tr.removeModelTitle": "Apagar o modelo {name} do disco ({size})",
  "tr.pageNoResponse": "a página não respondeu",

  // Busca no texto completo
  "ft.title": "Texto completo",
  "ft.noIndexPre":
    "Este arquivo ainda não tem índice de busca. O LocalZIM cria um índice local",
  "ft.noIndexStrong": "uma única vez",
  "ft.noIndexPost": "— pode demorar e ocupar espaço em disco, proporcional ao tamanho do arquivo.",
  "ft.buildNow": "Criar índice agora",
  "ft.indexing": "Indexando artigos… {pct}%",
  "ft.indexFail": "Falha na indexação — tente de novo.",
  "ft.searching": "Buscando…",
  "ft.nothing": "Nada encontrado.",

  // Idioma
  "lang.title": "Idioma",
} as const;

export type MessageKey = keyof typeof pt;

const en: Record<MessageKey, string> = {
  "lib.sub": "Your offline library — ZIM file reader (Wikipedia, Stack Overflow, Gutenberg…)",
  "lib.openBtn": "Open .zim file…",
  "lib.createFolder": "Create .zim from a folder…",
  "lib.createFolderTitle": "Packs a folder of HTML into a .zim file",
  "lib.createSite": "Create .zim from a site…",
  "lib.createSiteTitle": "Downloads a site (local crawler) and packs it into a .zim",
  "lib.open": "Open",
  "lib.recent": "Recent",
  "lib.closeFile": "Close this file",
  "lib.removeRecent": "Remove from recents",
  "lib.articles": "{n} articles",
  "lib.emptyPre": "No file open yet. Open a",
  "lib.emptyMid": "from your computer — you can download the whole Wikipedia, Wiktionary, Stack Overflow and much more at",
  "lib.defaultLibrary": "Library",
  "lib.defaultSite": "Site",

  "dlg.openZim": "Open ZIM file",
  "dlg.zimFiles": "ZIM files",
  "dlg.zimFile": "ZIM file",
  "dlg.sourceDir": "Folder with the content (HTML)",
  "dlg.saveZim": "Save ZIM file",

  "err.noSource": "Choose the source folder.",
  "err.noOutput": 'Choose where to save the .zim file ("Choose…" button under Save as).',
  "err.noUrl": "Enter the site address.",

  "site.title": "Create .zim from a site",
  "site.hintPre":
    "Local crawler: downloads pages starting from the initial URL (respecting robots.txt) with images, CSS and scripts, rewrites the links and packs it. Finds chapters listed in navigation JS (mdBook docs, e.g. the Rust book). Works well for documentation, blogs and wikis; JavaScript-built sites (SPAs) may come out incomplete — for those, use",
  "site.url": "Site address",
  "site.urlPlaceholder": "https://docs.example.com",
  "site.depth": "Link depth",
  "site.maxPages": "Max pages",
  "site.samePathPre": "Download only what's inside the initial path (e.g. started at",
  "site.samePathPost": ", won't leave it) — recommended",
  "site.crawling": "Downloading pages… {pages} (queue: {queued})",
  "site.doneBanner": "Done: {articles} pages, {size} — the file was opened in the library.",
  "site.download": "Download and create",

  "folder.title": "Create .zim from a folder",
  "folder.hintPre":
    "Packs a folder of HTML (a saved site, documentation, exported notes) into a",
  "folder.hintMid": "file. Relative links between pages keep working. To capture a site from the internet, use",
  "folder.hintPost": "(or",
  "folder.hintEnd": "and pack it here).",
  "folder.source": "Source folder",
  "folder.sourcePlaceholder": "choose the folder with the content",
  "folder.creator": "Creator",
  "folder.mainPage": "Main page",
  "folder.mainPlaceholder": "auto (index.html)",
  "folder.packing": "Packing… {pct}%",
  "folder.doneBanner": "Done: {articles} articles, {size} — the file was opened in the library.",
  "folder.create": "Create",

  "form.saveAs": "Save as",
  "form.outputPlaceholder": ".zim file destination",
  "form.choose": "Choose…",
  "form.title": "Title",
  "form.language": "Language",
  "form.description": "Description",
  "common.cancel": "Cancel",
  "common.close": "Close",

  "rd.library": "Library",
  "rd.back": "Back (Alt+←)",
  "rd.forward": "Forward (Alt+→)",
  "rd.home": "Main page",
  "rd.random": "Random article",
  "rd.searchPlaceholder": "Search in {name}… (Ctrl+K)",
  "rd.zoomOut": "Zoom out (Ctrl+-)",
  "rd.zoomIn": "Zoom in (Ctrl+=)",
  "rd.findTitle": "Find on page (Ctrl+F)",
  "rd.translateTitle": "Translate page (offline)",
  "rd.themeTitle": "Toggle light/dark theme",
  "rd.searchFt": 'Search “{q}” in the full text',
  "rd.iframeTitle": "ZIM file content",

  "find.placeholder": "Find on page…",
  "find.prev": "Previous (Shift+Enter)",
  "find.next": "Next (Enter)",
  "find.close": "Close (Esc)",

  "ext.title": "Leave the file",
  "ext.hintPre": "This link isn't part of the file",
  "ext.hintMid": "— it points to",
  "ext.hintPost": ", on the internet. Open it in your browser?",
  "ext.dontWarn": "Don't warn again while LocalZIM is open",
  "ext.openBrowser": "Open in browser",

  "tr.title": "Offline translation",
  "tr.translateTo": "Translate to",
  "tr.articleLang": "Article language",
  "tr.detected": "Detected: {name}",
  "tr.autoDetect": "Detect automatically",
  "tr.noDetect":
    "Couldn't detect this article's language — choose above. Translation works between Portuguese, Spanish and English.",
  "tr.already": "The article is already in {name}.",
  "tr.firstTimePre": "First time in this direction: LocalZIM downloads the translation model",
  "tr.firstTimeStrong": "just once",
  "tr.firstTimePost": "and then works 100% offline.",
  "tr.viaEnglish": " Portuguese ↔ Spanish goes through English, so it's two models.",
  "tr.preparing": "preparing…",
  "tr.downloadModels": "Download models ({size})",
  "tr.downloadModel": "Download model ({size})",
  "tr.translatePage": "Translate page",
  "tr.loadingModel": "Loading model…",
  "tr.translating": "Translating… {done}/{total} blocks",
  "tr.stop": "Stop",
  "tr.doneCached": "✓ Page translated (it's cached — coming back here is instant).",
  "tr.showTranslation": "Show translation",
  "tr.showOriginal": "Show original",
  "tr.autoNext": "Translate the next pages automatically",
  "tr.removeModelTitle": "Delete the {name} model from disk ({size})",
  "tr.pageNoResponse": "the page didn't respond",

  "ft.title": "Full text",
  "ft.noIndexPre": "This file doesn't have a search index yet. LocalZIM builds a local index",
  "ft.noIndexStrong": "just once",
  "ft.noIndexPost": "— it may take a while and use disk space, proportional to the file size.",
  "ft.buildNow": "Build index now",
  "ft.indexing": "Indexing articles… {pct}%",
  "ft.indexFail": "Indexing failed — try again.",
  "ft.searching": "Searching…",
  "ft.nothing": "Nothing found.",

  "lang.title": "Language",
};

const es: Record<MessageKey, string> = {
  "lib.sub": "Tu biblioteca offline — lector de archivos ZIM (Wikipedia, Stack Overflow, Gutenberg…)",
  "lib.openBtn": "Abrir archivo .zim…",
  "lib.createFolder": "Crear .zim de una carpeta…",
  "lib.createFolderTitle": "Empaqueta una carpeta con HTML en un archivo .zim",
  "lib.createSite": "Crear .zim de un sitio…",
  "lib.createSiteTitle": "Descarga un sitio (crawler local) y lo empaqueta en un .zim",
  "lib.open": "Abiertos",
  "lib.recent": "Recientes",
  "lib.closeFile": "Cerrar este archivo",
  "lib.removeRecent": "Quitar de recientes",
  "lib.articles": "{n} artículos",
  "lib.emptyPre": "Aún no hay ningún archivo abierto. Abre un",
  "lib.emptyMid": "de tu ordenador — puedes descargar toda la Wikipedia, Wikcionario, Stack Overflow y mucho más en",
  "lib.defaultLibrary": "Biblioteca",
  "lib.defaultSite": "Sitio",

  "dlg.openZim": "Abrir archivo ZIM",
  "dlg.zimFiles": "Archivos ZIM",
  "dlg.zimFile": "Archivo ZIM",
  "dlg.sourceDir": "Carpeta con el contenido (HTML)",
  "dlg.saveZim": "Guardar archivo ZIM",

  "err.noSource": "Elige la carpeta de origen.",
  "err.noOutput": 'Elige dónde guardar el archivo .zim (botón "Elegir…" en Guardar como).',
  "err.noUrl": "Indica la dirección del sitio.",

  "site.title": "Crear .zim de un sitio",
  "site.hintPre":
    "Crawler local: descarga las páginas desde la URL inicial (respetando robots.txt) con imágenes, CSS y scripts, reescribe los enlaces y lo empaqueta. Encuentra capítulos listados en JS de navegación (docs mdBook, ej.: el libro de Rust). Funciona bien para documentación, blogs y wikis; los sitios montados con JavaScript (SPA) pueden salir incompletos — para esos, usa",
  "site.url": "Dirección del sitio",
  "site.urlPlaceholder": "https://docs.ejemplo.com",
  "site.depth": "Profundidad de enlaces",
  "site.maxPages": "Máximo de páginas",
  "site.samePathPre": "Descargar solo lo que está dentro de la ruta inicial (ej.: empezó en",
  "site.samePathPost": ", no sale de ella) — recomendado",
  "site.crawling": "Descargando páginas… {pages} (cola: {queued})",
  "site.doneBanner": "Listo: {articles} páginas, {size} — el archivo ya se abrió en la biblioteca.",
  "site.download": "Descargar y crear",

  "folder.title": "Crear .zim de una carpeta",
  "folder.hintPre":
    "Empaqueta una carpeta con HTML (un sitio guardado, documentación, notas exportadas) en un archivo",
  "folder.hintMid": ". Los enlaces relativos entre las páginas siguen funcionando. Para capturar un sitio de internet, usa",
  "folder.hintPost": "(o",
  "folder.hintEnd": "y empaquétalo aquí).",
  "folder.source": "Carpeta de origen",
  "folder.sourcePlaceholder": "elige la carpeta con el contenido",
  "folder.creator": "Creador",
  "folder.mainPage": "Página inicial",
  "folder.mainPlaceholder": "auto (index.html)",
  "folder.packing": "Empaquetando… {pct}%",
  "folder.doneBanner": "Listo: {articles} artículos, {size} — el archivo ya se abrió en la biblioteca.",
  "folder.create": "Crear",

  "form.saveAs": "Guardar como",
  "form.outputPlaceholder": "destino del archivo .zim",
  "form.choose": "Elegir…",
  "form.title": "Título",
  "form.language": "Idioma",
  "form.description": "Descripción",
  "common.cancel": "Cancelar",
  "common.close": "Cerrar",

  "rd.library": "Biblioteca",
  "rd.back": "Atrás (Alt+←)",
  "rd.forward": "Adelante (Alt+→)",
  "rd.home": "Página principal",
  "rd.random": "Artículo aleatorio",
  "rd.searchPlaceholder": "Buscar en {name}… (Ctrl+K)",
  "rd.zoomOut": "Reducir zoom (Ctrl+-)",
  "rd.zoomIn": "Aumentar zoom (Ctrl+=)",
  "rd.findTitle": "Buscar en la página (Ctrl+F)",
  "rd.translateTitle": "Traducir página (offline)",
  "rd.themeTitle": "Alternar tema claro/oscuro",
  "rd.searchFt": 'Buscar “{q}” en el texto completo',
  "rd.iframeTitle": "Contenido del archivo ZIM",

  "find.placeholder": "Buscar en la página…",
  "find.prev": "Anterior (Shift+Enter)",
  "find.next": "Siguiente (Enter)",
  "find.close": "Cerrar (Esc)",

  "ext.title": "Salir del archivo",
  "ext.hintPre": "Este enlace no forma parte del archivo",
  "ext.hintMid": "— apunta a",
  "ext.hintPost": ", en internet. ¿Abrirlo en tu navegador?",
  "ext.dontWarn": "No avisar de nuevo mientras LocalZIM esté abierto",
  "ext.openBrowser": "Abrir en el navegador",

  "tr.title": "Traducción offline",
  "tr.translateTo": "Traducir a",
  "tr.articleLang": "Idioma del artículo",
  "tr.detected": "Detectado: {name}",
  "tr.autoDetect": "Detectar automáticamente",
  "tr.noDetect":
    "No se pudo detectar el idioma de este artículo — elígelo arriba. La traducción funciona entre portugués, español e inglés.",
  "tr.already": "El artículo ya está en {name}.",
  "tr.firstTimePre": "Primera vez en esta dirección: LocalZIM descarga el modelo de traducción",
  "tr.firstTimeStrong": "una sola vez",
  "tr.firstTimePost": "y luego funciona 100% offline.",
  "tr.viaEnglish": " Portugués ↔ español pasa por el inglés, así que son dos modelos.",
  "tr.preparing": "preparando…",
  "tr.downloadModels": "Descargar modelos ({size})",
  "tr.downloadModel": "Descargar modelo ({size})",
  "tr.translatePage": "Traducir página",
  "tr.loadingModel": "Cargando modelo…",
  "tr.translating": "Traduciendo… {done}/{total} bloques",
  "tr.stop": "Parar",
  "tr.doneCached": "✓ Página traducida (queda en caché — volver aquí es instantáneo).",
  "tr.showTranslation": "Ver traducción",
  "tr.showOriginal": "Ver original",
  "tr.autoNext": "Traducir las próximas páginas automáticamente",
  "tr.removeModelTitle": "Borrar el modelo {name} del disco ({size})",
  "tr.pageNoResponse": "la página no respondió",

  "ft.title": "Texto completo",
  "ft.noIndexPre": "Este archivo aún no tiene índice de búsqueda. LocalZIM crea un índice local",
  "ft.noIndexStrong": "una sola vez",
  "ft.noIndexPost": "— puede tardar y ocupar espacio en disco, proporcional al tamaño del archivo.",
  "ft.buildNow": "Crear índice ahora",
  "ft.indexing": "Indexando artículos… {pct}%",
  "ft.indexFail": "Error en la indexación — inténtalo de nuevo.",
  "ft.searching": "Buscando…",
  "ft.nothing": "Nada encontrado.",

  "lang.title": "Idioma",
};

const DICTS: Record<Locale, Record<MessageKey, string>> = { pt, en, es };

/** Palpite de locale pelo idioma do sistema (só no 1º uso). */
export function detectLocale(): Locale {
  const l = (typeof navigator !== "undefined" ? navigator.language : "pt").toLowerCase();
  if (l.startsWith("en")) return "en";
  if (l.startsWith("es")) return "es";
  return "pt";
}

function loadLocale(): Locale {
  const v = typeof localStorage !== "undefined" ? localStorage.getItem(LOCALE_KEY) : null;
  return v === "pt" || v === "en" || v === "es" ? v : detectLocale();
}

let current: Locale = loadLocale();
const listeners = new Set<() => void>();

export function getLocale(): Locale {
  return current;
}

export function setLocale(locale: Locale) {
  if (locale === current) return;
  current = locale;
  try {
    localStorage.setItem(LOCALE_KEY, locale);
  } catch {
    /* localStorage indisponível */
  }
  for (const l of listeners) l();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** Inscreve o componente nas trocas de locale. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, getLocale);
}

/** BCP-47 do locale atual (pra Number/Date toLocaleString). */
export function localeTag(): string {
  return current === "pt" ? "pt-BR" : current === "es" ? "es-ES" : "en-US";
}

/** Traduz uma chave, interpolando placeholders `{param}`. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  let msg: string = DICTS[current][key] ?? pt[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      msg = msg.split(`{${k}}`).join(String(v));
    }
  }
  return msg;
}
