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
  quando existe), zoom por livro e **modo escuro** aplicado também dentro do artigo.
- Links externos abrem no navegador do sistema; associação de arquivo `.zim`; instância única
  (clique duplo num `.zim` reaproveita a janela aberta).

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

## Limitações conhecidas (v0.1)

- **Sem busca full-text**: o índice Xapian embutido nos ZIM não é lido; a busca é por
  prefixo de título (como as sugestões do Kiwix).
- Vídeos tocam, mas **sem seek** (o protocolo ainda não responde requisições Range).
- Clusters sem compressão são lidos inteiros por requisição (arquivos com vídeos grandes
  podem gastar memória; há cache LRU de 12 clusters).
- Voltar/avançar usa o histórico do webview; ao alternar entre vários livros abertos o
  histórico é compartilhado entre eles.
