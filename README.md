# LocalZIM

Leitor de bibliotecas **ZIM** **100% offline** — Wikipédia inteira, Wikcionário,
Stack Overflow, Projeto Gutenberg e qualquer outro arquivo de
[library.kiwix.org](https://library.kiwix.org) no seu computador, sem internet.

> **Inspirado no [Kiwix](https://kiwix.org)** — o projeto que criou o formato ZIM e leva
> conteúdo offline para o mundo inteiro. O LocalZIM é uma reimplementação independente do
> leitor (Rust puro + Tauri); todo o crédito do formato e do ecossistema de conteúdo é do
> time do [openZIM/Kiwix](https://github.com/openzim).

Parte da suíte **Local** (Tauri 2 + React 19 + TypeScript + Rust). Licença MIT.

## Funcionalidades

- **Parser ZIM em Rust puro** (`src-tauri/src/zim.rs`) — sem libzim/C++: cabeçalho, dirents,
  busca binária por URL/título, clusters **zstd**, **LZMA2/XZ** e sem compressão, redirects,
  metadados (título, descrição, idioma, ilustração) e os dois esquemas de namespace
  (antigo `A/I/M/-` e novo `C/M/W/X`).
- **Conteúdo servido direto ao WebView2** (WebView nativo do Windows; WebKitGTK no Linux) pelo
  protocolo customizado `zim://` — `/<id>/<N>/<url>` — com links relativos funcionando de graça
  e redirects do ZIM virando `302`.
- **Biblioteca** com recentes persistidos (nome, descrição, idioma, tamanho, ícone do arquivo).
- **Leitor** com voltar/avançar, página principal, **artigo aleatório**, **busca por título** com
  sugestões (prefixo + variantes de capitalização, usa o índice `X/listing/titleOrdered/v1`
  quando existe), zoom por livro, **localizar na página (Ctrl+F)** e **modo escuro** aplicado
  também dentro do artigo.
- **Busca no texto completo** (`src-tauri/src/search.rs`): o índice Xapian embutido nos ZIM
  exige C++ e não é lido; em vez disso o LocalZIM constrói o próprio índice **tantivy** — uma
  única vez, em segundo plano, com progresso e cancelamento — guardado na pasta de dados do
  app (chaveado pelo UUID do arquivo). BM25 com boost de título, busca insensível a acentos
  ("sao paulo" acha "São Paulo") e trechos com os termos destacados.
- **Tradução offline de artigos** (pt-BR · es · en): modelos Marian/OPUS-MT no candle, na CPU,
  baixados uma vez por direção — detalhes na seção abaixo.
- **Requisições Range** no protocolo (`206 Partial Content`) — vídeo e áudio com seek; clusters
  sem compressão são lidos por fatia (vídeo grande não carrega o cluster inteiro na RAM).
- Links externos (fora do arquivo) mostram um **aviso de "sair do arquivo"** com o destino antes
  de abrir no navegador do sistema — com opção de não avisar de novo na sessão. Associação de
  arquivo `.zim`; instância única (clique duplo num `.zim` reaproveita a janela aberta).
- **Criar .zim de uma pasta** (`src-tauri/src/zimwriter.rs`): empacota uma pasta com HTML
  (site salvo com `wget --mirror`, documentação, notas exportadas) num `.zim` válido — título
  extraído de cada página, metadados, favicon, clusters zstd pra texto e crus pra mídia
  (streaming: vídeo grande não passa pela RAM), md5 no rodapé. Abre no LocalZIM e no Kiwix.
  Para rastrear um site da internet o [zimit](https://github.com/openzim/zimit) continua sendo
  a ferramenta certa; o criador local é o caminho rápido quando você já tem os arquivos.
- **Criar .zim de um site** (`src-tauri/src/crawler.rs`): crawler estático local — BFS com
  limite de profundidade/páginas, por padrão **restrito ao caminho da URL inicial** (começou em
  `/book/`, não sai dele; desmarcável pra pegar o domínio todo), respeita robots.txt, intervalo
  educado entre requisições, baixa CSS/JS/imagens/fontes (inclusive de CDN, em `_ext/`), segue
  `url()` e `@import` dos CSS, **descobre capítulos listados em JS de navegação** (o `toc.js`
  dos mdBook — livro do Rust etc.) e reescreve todos os links pra caminhos relativos (externos
  ficam absolutos e abrem no navegador). Ótimo pra documentação, blogs e wikis; sites montados
  por JavaScript (SPA) podem sair incompletos — pra esses, zimit.

## Tradução offline (pt-BR · es · en)

O leitor traduz artigos inteiros **sem internet**, com modelos
[Marian/OPUS-MT da Helsinki-NLP](https://huggingface.co/Helsinki-NLP) rodando no
[candle](https://github.com/huggingface/candle) (CPU, dentro do próprio app — sem
Python nem runtime externo):

- Botão **🌐** na toolbar: escolha o idioma alvo (português · español · english); o idioma
  do artigo é detectado pelo metadado do livro/página, com ajuste manual se precisar.
- Cada direção baixa **um modelo uma única vez** (289–465 MB, hospedados nas releases do
  [LocalZIM-models](https://github.com/Anon5T4R/LocalZIM-models), sha256 conferido).
  pt ↔ es não tem modelo dedicado — o app **pivota pelo inglês** (dois modelos).
- en → pt usa o `opus-mt-tc-big-en-pt` com alvo `>>pob<<` (**português brasileiro**).
- A tradução troca os blocos de texto do artigo progressivamente, com barra de progresso;
  **"ver original"** restaura na hora (a marcação inline do bloco traduzido vira texto
  simples — links dentro de parágrafos traduzidos deixam de ser clicáveis).
- Resultado fica em **cache por artigo/direção** (revisitar é instantâneo) e há opção de
  traduzir as próximas páginas automaticamente. Modelos saem da RAM após ~5 min sem uso e
  podem ser apagados do disco pelo próprio painel.

## Onde conseguir (e como criar) arquivos .zim

- **Baixar prontos:** [library.kiwix.org](https://library.kiwix.org) — Wikipédia (todas as
  línguas e recortes), Wikcionário, Wikivoyage, Stack Overflow, Gutenberg, TED, DevDocs…
- **Criar os seus:** [zimit](https://github.com/openzim/zimit) transforma qualquer site em um
  `.zim` (crawler + empacotador). Tem versão hospedada em
  [zimit.kiwix.org](https://zimit.kiwix.org) — cola a URL e recebe o arquivo pronto — e a
  ferramenta em Docker para rodar local. Para converter outros conteúdos, veja as demais
  ferramentas do [openZIM](https://github.com/openzim).

## Atalhos

| Atalho | Ação |
|---|---|
| `Alt+←` / `Alt+→` | Voltar / avançar |
| `Ctrl+K` | Focar a busca |
| `Ctrl+F` | Localizar na página |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Zoom (aumenta / diminui / 100%) |

Funcionam inclusive com o foco dentro do artigo (a ponte injetada encaminha pro app).

## Desenvolvimento

```bash
npm install
npm run tauri dev   # porta 1440 (HMR 1441)
```

Testes: `npm test` (front) e `cargo test` em `src-tauri/` (parser ZIM com arquivo sintético).

## Release

Padrão da suíte: bump de versão em `package.json` + `src-tauri/tauri.conf.json` +
`src-tauri/Cargo.toml`, tag `vX.Y.Z`, push — o GitHub Actions builda NSIS (Windows) e
AppImage (Linux) e publica a release.

## Limitações conhecidas

- A busca no texto completo exige **criar o índice local uma vez** (botão no painel de busca);
  para um ZIM gigante isso demora e ocupa espaço em disco proporcional ao arquivo. O índice
  Xapian que já vem dentro do ZIM não é aproveitado (só é legível via libzim/C++).
- Voltar/avançar usa o histórico do webview; ao alternar entre vários livros abertos o
  histórico é compartilhado entre eles.
