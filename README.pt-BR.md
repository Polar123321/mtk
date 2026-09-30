<div align="center">

<img src="assets/banner.svg" alt="MTK — Toda saída de ferramenta. Muito menos tokens." width="100%">

<br>

[![CI](https://github.com/Polar123321/mtk/actions/workflows/ci.yml/badge.svg)](https://github.com/Polar123321/mtk/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-2dd4bf.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-f74c00.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Claude Code](https://img.shields.io/badge/Claude_Code-hook-7ee787.svg)](#instalar-claude-code)
[![MCP](https://img.shields.io/badge/MCP-proxy-60a5fa.svg)](#codex-e-qualquer-cliente-mcp)

[English](README.md) · **Português**

</div>

---

**MTK** é um único binário em Rust que reduz o que o seu agente de código **lê**: a saída de **comandos de shell, servidores MCP (incluindo o Roblox Studio), grep e arquivos grandes**, antes de chegar ao modelo. A mesma informação com muito menos tokens, e o original intacto está sempre a um comando de distância.

```text
   saída da ferramenta ──▶  MTK  ──▶  modelo
   10.765 tokens                     903 tokens       (resultado real do Roblox Studio)
                            │
                            └── original guardado em disco:  mtk recall <id>
```

## Veja funcionando

Um resultado real de `search_game_tree` do Roblox Studio. A ferramenta devolve um array JSON plano em que `name` e `parentName` podem ser deduzidos de `fullPath`:

<table>
<tr><th>O que a ferramenta devolveu · 2.126 tokens</th><th>O que o modelo lê · 279 tokens</th></tr>
<tr><td>

```json
[{"parentName":"Game","fullPath":"Workspace",
  "name":"Workspace","className":"Workspace"},
 {"parentName":"Workspace",
  "fullPath":"Workspace.OpenWorldMap",
  "name":"OpenWorldMap","className":"Folder"},
 {"parentName":"Workspace",
  "fullPath":"Workspace.TrainMissionSlots",
  "name":"TrainMissionSlots","className":"Folder"},
 {"parentName":"TrainMissionSlots",
  "fullPath":"Workspace.TrainMissionSlots.Slot1",
  "name":"Slot1","className":"Folder"},
 … mais 36 nós assim
```

</td><td>

```text
# name:Class, indent = child
Workspace:Workspace
 OpenWorldMap:Folder
  ParkourRoutes:Folder
  NPCs:Folder
  [Model] Expansion, OpenWorld_City2,
          OpenWorld_City
 TrainMissionSlots:Folder
  [Folder] Slot1, Slot2, Slot3
 TRexForest:Folder
  [Folder] Clearing, Chase1, Ravine, …
 Combat Animations:Model
  Humanoid:Humanoid
  [Part] Torso, HumanoidRootPart, …
```

</td></tr>
</table>

Um log de build com 300 linhas vira três:

```text
   Compiling crate1 v0.1.0
… [298 more similar build-progress lines]
   Compiling crate300 v0.300.0
[mtk -98% · full: mtk recall 30a5ee12]
```

## Resultados

A contagem de tokens é uma estimativa do próprio MTK (trechos alfanuméricos ≈ 4 caracteres/token, cada pontuação 1), então trate as porcentagens como referência. As duas primeiras linhas são **capturas reais**; o resto são fixtures **sintéticas** geradas por [`scripts/gen-fixtures.js`](scripts/gen-fixtures.js): mostram o mecanismo, não a sua média.

| Fixture | Origem | Antes | Depois | Economia |
|---|---|---:|---:|---:|
| Roblox `search_game_tree` | captura real | 2.126 | 279 | **86,9%** |
| Roblox `inspect_instance` | captura real | 644 | 145 | **77,5%** |
| Roblox `get_console_output` · `script_search` | captura real | 254 | 253 | 0,4% |
| Log de npm install | sintética | 2.711 | 113 | 95,8% |
| Log de aplicação com repetições e um crash | sintética | 13.140 | 845 | 93,6% |
| `cargo build` com um erro | sintética | 2.160 | 214 | 90,1% |
| Lista de issues estilo GitHub (MCP) | sintética | 19.429 | 2.552 | 86,9% |
| `pytest` com uma falha | sintética | 1.052 | 518 | 50,8% |

Saídas pequenas e sem redundância são deixadas em paz. Essa é a linha de 0,4%, e é o comportamento correto.

### Medido com o `usage` real da API

Claude Code headless, a mesma tarefa com o MTK ligado e desligado. É a contagem de tokens da própria API, não uma estimativa.

| Tarefa | Sem MTK | Com MTK |
|---|---:|---:|
| Roblox `search_game_tree` (profundidade 4): tokens escritos no cache | 23.823 · 19.372 | **13.771 · 13.455** |
| … custo | $0,061 · $0,052 | **$0,040 · $0,040** |
| "O que faz `Hir::concat`?" num arquivo de 3.873 linhas: tokens escritos no cache | 40.696 · 36.588 | **14.337 · 14.334** |
| … custo | $0,099 · $0,089 | **$0,041 · $0,041** |

Duas rodadas por lado, uma tarefa cada: consistente, mas é uma demonstração e não um benchmark. O percentual por sessão é diluído pelo prompt de sistema fixo; os resultados das ferramentas em si encolhem muito mais. Rode `mtk gain` depois de alguns dias para ver o **seu** número.

## Como funciona

<div align="center">
<img src="assets/flow.svg" alt="A saída da ferramenta entra no pipeline do MTK e um resultado bem menor chega ao modelo" width="100%">
</div>

O MTK usa o hook **`PostToolUse`**. O Claude Code deixa esse hook substituir o resultado de *qualquer* ferramenta (`updatedToolOutput`) mantendo a estrutura, então o MTK nunca reescreve seus comandos e nunca quebra aspas, pipes ou heredocs.

| Etapa | O que faz |
|---|---|
| `normalize` | remove ANSI, quadros de barra de progresso com `\r`, espaços no fim, sequências de linhas em branco |
| `json` | poda valores nulos/vazios, arredonda floats de 17 dígitos, mostra arrays de objetos como tabelas (`key[N]{cols}:`), içar colunas constantes e prefixos de URL, achata objetos aninhados, remove ruído de links REST |
| `roblox-tree` | `search_game_tree` → árvore indentada |
| `roblox-inspect` | `inspect_instance` → só as propriedades não-default, mais a contagem do que foi omitido |
| `log` | colapsa sequências de ruído (`Compiling…`, `PASS…`, progresso), linhas quase idênticas (`… [N more similar]`) e repetições exatas distantes (`[×N]`) |
| `grep` | `caminho:linha:texto` reagrupado por arquivo, raiz comum dita uma vez |
| `budget` | teto duro de tokens (padrão 4.000): cabeça + cauda, com **linhas de erro e aviso resgatadas** do meio |
| `recall` | o original intacto é guardado; o rodapé diz ao modelo como buscá-lo |

### Guard de leitura: ler só o necessário

Comprimir saídas não resolve o maior desperdício de entrada: ler um arquivo de 4.000 linhas para usar 30. Um hook `PreToolUse` no `Read` nega a **primeira** leitura de arquivo inteiro de um código grande e devolve um outline, para que o agente releia só o trecho de que precisa. Repita o mesmo `Read` e ele passa, então leituras completas legítimas continuam funcionando.

```text
mtk: hir.rs has 3873 lines (~55286 tokens). Outline (start-end kind name):
213-252 impl Hir
  215-220 fn kind
253-716 impl Hir
  258-278 fn empty
  439-571 fn concat        ◀ o agente lê offset=439 limit=133
  …
```

`mtk outline <arquivo>` imprime o mesmo. Suporta Rust, Python, Lua/Luau, JS/TS, Go, Markdown, Java/Kotlin/C#/C/C++/Swift (heurística por linha, sem parser). Arquivos com menos de 3 símbolos (minificados, dados, logs, JSON) nunca são barrados.

## Instalar (Claude Code)

```bash
git clone https://github.com/Polar123321/mtk && cd mtk
cargo install --path .        # coloca o `mtk` no seu PATH

mtk install --dry-run         # mostra exatamente o que mudaria em ~/.claude/settings.json
mtk install                   # adiciona os hooks (faz backup do settings.json antes)
mtk doctor                    # verifica
```

Hooks adicionados: `PostToolUse` em `Bash|PowerShell|Grep|WebFetch|mcp__.*` e `PreToolUse` em `Read`. `mtk uninstall` remove só as entradas do MTK. Convive com o [RTK](https://github.com/rtk-ai/rtk): o RTK reduz a saída do comando primeiro, o MTK processa o que voltar e cobre o que um hook só de Bash não enxerga.

```bash
mtk gain                      # quanto você economizou até agora
mtk recall <id> --grep ERROR  # busca a saída original por trás de um rodapé
mtk bench tests/fixtures      # tabela de economia sobre saídas salvas
```

## Codex e qualquer cliente MCP

O `PostToolUse` do Codex não consegue reescrever a saída (segundo a doc dele), mas servidores MCP podem ser envolvidos. O `mtk mcp-proxy` é um proxy stdio transparente para qualquer servidor MCP: repassa cada byte e reescreve apenas os resultados de `tools/call`, nunca resultados de erro nem ferramentas de leitura de arquivo.

```bash
mtk mcp status RobloxStudio    # [mcp_servers.RobloxStudio] está envolvido?
mtk mcp wrap   RobloxStudio    # ~/.codex/config.toml passa a iniciar: mtk mcp-proxy -- <comando original>
mtk mcp unwrap RobloxStudio    # restaura exatamente (uma cópia .mtk.bak é feita a cada mudança)
```

Testado contra o servidor MCP real do Roblox Studio ([`scripts/mcp-e2e.js`](scripts/mcp-e2e.js)): as mesmas 28 ferramentas, resultados pequenos idênticos byte a byte, um resultado real de `search_game_tree` com −75,8% em bytes. Reinicie o Codex depois de envolver. A saída de shell dentro do Codex **não** é coberta: exigiria reescrever o comando no `PreToolUse`, a abordagem frágil que o MTK evita de propósito.

## MTK e RTK

O [RTK](https://github.com/rtk-ai/rtk) é um projeto maduro, com mais de 100 filtros de comando escritos à mão e suporte a vários agentes, e é excelente no que faz. O MTK não substitui essa amplitude; ele ataca outra camada.

| | RTK | MTK |
|---|---|---|
| Mecanismo | reescreve o comando Bash (`PreToolUse`) | reescreve o *resultado* (`PostToolUse`) |
| Cobre | comandos Bash | Bash, PowerShell, Grep, WebFetch, **qualquer ferramenta MCP** |
| Escolhe o filtro por | nome do comando | tipo de conteúdo (JSON, árvore, log, grep…) |
| Leitura de arquivo grande | — | guard de outline no `Read` |
| Agentes | vários | Claude Code (hook) · qualquer cliente MCP (proxy) |
| Inteligência por comando | 100+ filtros | poucos (ruído de build, teste, gerenciadores de pacote) |

Combinam bem: use os dois.

## Segurança

- **Falha aberta.** Qualquer erro de parse, pânico ou dúvida não produz saída, e o Claude Code mantém o original.
- **Nunca pior.** Se o resultado não for menor que a entrada, a entrada é usada.
- **Conteúdo de arquivo é sagrado.** `Read`, `Edit`, `Write`, `Glob`, ferramentas MCP como `read_file` / `script_read` / `get_file…` e comandos como `cat` / `head` / `Get-Content` nunca são reescritos: bytes exatos importam para edição.
- **Erros sobrevivem ao corte.** O budget resgata linhas de erro e aviso do meio.
- **Imagens e blocos binários** passam intactos.
- **Nada se perde de verdade.** `mtk recall` devolve o original por 14 dias / 500 entradas.
- **Os testes não tocam nos seus dados.** 46 testes, incluindo as capturas reais do Roblox e uma checagem de que nomes e classes sobrevivem à conversão da árvore.

## Configuração

`~/.mtk/config.toml`, tudo opcional:

```toml
enabled = true
min_tokens = 60             # ignora saídas menores que isso
max_tokens = 4000           # orçamento duro por saída
min_gain_pct = 8.0          # pula reescritas que economizam menos que isso
footer = true               # a linha "[mtk -87% · full: mtk recall <id>]"
store = true                # guarda os originais para `mtk recall`
json = true
drop_noise_keys = true      # ruído de node_id / *_url em objetos estilo REST
read_guard = true
read_guard_min_lines = 500
float_decimals = 5
exclude_tools = []          # ex.: ["mcp__figma"]
```

`MTK_DISABLE=1` desliga por uma execução; `MTK_HOME` muda a pasta de dados.

## Limitações honestas

- A contagem de tokens é uma estimativa, não um tokenizer.
- `json`, `log` e `budget` têm **perda por design** (valores vazios, ruído de links REST, dígitos de float, sequências colapsadas). O original é sempre recuperável, mas o modelo precisa decidir pedir.
- A tabela de propriedades default por trás do `roblox-inspect` é pequena e independente de classe; uma classe cujo default seja diferente mostraria uma propriedade como oculta. A saída diz quantas foram omitidas.
- Verificado no Claude Code 2.1.x no Windows. Os formatos de payload do hook (`tool_response`, `updatedToolOutput`) foram capturados do sistema real; hooks de outros agentes não são suportados.
- O CI compila, testa e roda o bench em Linux e Windows (verde na primeira execução); as medições com Studio e API reais acima foram feitas localmente, então não fazem parte do CI.
- Os resultados do `Read` e do `Glob` embutidos são deliberadamente não comprimidos.

## Desenvolver

```bash
cargo test                     # 46 testes
node scripts/gen-fixtures.js   # regenera as fixtures sintéticas
mtk bench tests/fixtures
mtk filter alguma-saida.txt    # veja exatamente o que o modelo receberia
```

Mapa do código: `src/compress/*` (o pipeline), `src/hook.rs` (hook do Claude Code + `transform`, independente de agente), `src/mcp_proxy.rs`, `src/readguard.rs` + `src/outline.rs`, `src/install.rs`, `src/codex.rs`.

## Licença

[MIT](LICENSE)
