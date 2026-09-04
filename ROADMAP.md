# Zerem — Roadmap

Cliente BitTorrent nativo para Windows (e Linux depois). **Rust + Slint**, um
processo, sem WebView.

A tese do produto cabe em uma frase: **leveza, performance e UX são o produto;
features são o que se acrescenta depois sem estragar as três.** Todo item deste
roadmap é julgado por isso, nessa ordem.

Detalhe técnico das decisões: [ARCHITECTURE.md](ARCHITECTURE.md).

---

## Princípio de execução

1. **Cada fase termina num binário que dá para usar de verdade.** Nada de fase
   que entrega "infraestrutura" e nada mais.
2. **Risco primeiro.** As duas incógnitas do projeto (a tabela no Slint e o
   throughput da engine) são resolvidas na Fase 0, com código descartável, antes
   de existir arquitetura para elas derrubarem.
3. **O escopo do MVP é fechado.** Feature nova antes da Fase 4 entra no backlog,
   não no código. A régua: *um cliente torrent que não faz isso é inutilizável?*
   Se a resposta for "não, mas seria bom", é backlog.
4. **Prosa em pt-BR, código e comentários em inglês** — igual a Clipo e Vayou.

---

## Alvos de aceite

Números, não adjetivos. Medidos na Fase 3 e verificados no CI a partir da Fase 4.
Se um alvo não for atingido, ele vira bug bloqueante — não vira alvo novo.

A coluna **medido** vem da Fase 0 ([docs/spike-report.md](docs/spike-report.md)),
com o renderizador por software. Os alvos de memória foram revisados *para baixo*
depois de medir: o piso não era a lista, era o renderizador. O tamanho do binário
é a exceção: ele é medido a cada build de release, e a linha abaixo é a de hoje.

| Métrica | Alvo | Medido (Fase 0) |
|---|---|---|
| Binário (exe, sem instalador) | ≤ 16 MB | **15,13 MB** ⚠️ (era 13,32 na Fase 1) |
| RAM ociosa — 10 torrents parados | ≤ 60 MB WS | **31,9 MB** ✅ (a 2000) |
| RAM — 1000 torrents na lista | ≤ 80 MB WS | **31,9 MB** ✅ (a 2000) |
| RAM — semeando 20 torrents ativos | ≤ 100 MB WS | — |
| CPU ocioso, janela aberta | ≤ 0,5 % | **0,016 %** ✅ |
| CPU ocioso, minimizado na bandeja | ≤ 0,1 % | **0,000 %** ✅ |
| Cold start até a janela pintada | ≤ 300 ms | — |
| CPU baixando | ≤ 1,5 % de um núcleo por MB/s | **1,45 %** ✅ |
| **CPU semeando** | ≤ 1 % de um núcleo | **0,77 %** ✅ |
| Custo do tick, 2000 torrents | ≤ 1 ms | **70 – 176 µs** ✅ |
| Scroll com 2000 torrents | 60 fps sem queda | ⏳ verificação manual |
| Resposta de qualquer clique | < 100 ms visível | ✅ atualização otimista |

O binário é o número a vigiar: 15,13 MB deixa menos de 1 MB de folga, e o custo
não foi a sparkline — trocá-la por um retângulo devolve 10 KB. A conta subiu ao
longo das Fases 1 e 2, e caber no orçamento por pouco é caber. Fechar a Fase 3
com uma medição de onde os 15 MB estão é trabalho da tabela de benchmarks.

Dois desses merecem destaque porque quase todo cliente falha neles: **minimizado
na bandeja o app não deve renderizar nada** (não é "renderizar pouco" — é parar o
tick de UI, e a Fase 0 mediu 0,000 % com nada mudando), e **1000 torrents na
lista** é onde clientes em Electron passam de 600 MB.

---

## Fase 0 — Provas de risco

**Objetivo:** matar as incógnitas antes de escrever arquitetura em cima delas.
Código descartável em [`spikes/`](spikes/) — só o `model.rs`, `sort.rs`, `fmt.rs`
e o `theme.slint` migram.

| # | Prova | Critério | Status |
|---|---|---|---|
| S0.1 | **Tabela Slint** — 2000 linhas, 1 Hz, ordenação, seleção múltipla, colunas redimensionáveis | 60 fps no scroll, ≤ 1 % CPU parado, seleção e scroll preservados | ✅ **aprovado**, falta rolar à mão |
| S0.3 | **Tick** — `Arc<Snapshot>` + modelo custom com `row_changed` pontual | Zero realocação por tick; diff de 2000 linhas < 1 ms | ✅ **aprovado** — 92 µs, `reset` nunca |
| S0.2 | **Engine** — librqbit baixando um ISO grande, **uTP habilitado** (atrás de flag, desligado por padrão), IPv6 ligado | Satura o link; uTP confirmado por `live_utp > 0`; latência intacta ao semear | ✅ **aprovado** — 3,72 GB em 96 s, pico 66,9 MB/s, 15 peers uTP |

**Portões de decisão — os dois fechados.** S0.1 passou, então o Slint fica.
S0.2 passou, então o librqbit fica: o plano B (**libtorrent-rasterbar via `cxx`**)
está descartado, e com ele o C++ no build. Nenhum dos dois se revisita.

**A Fase 0 acabou.** O que resta são verificações humanas — rolar a lista à mão,
e navegar enquanto semeia com o uplink cheio para ver o LEDBAT ceder a fila.

**Quatro decisões que a Fase 0 já mudou** — detalhe em
[docs/spike-report.md](docs/spike-report.md):

1. **Renderizador por software passa a ser o padrão.** Contra o femtovg: 86 % menos
   memória privada (17 MB vs 120 MB) e 5× menos CPU sob carga. A repintura
   parcial vence numa tabela em que 26 de 2000 linhas mudam por segundo.
2. **A ordenação era o gargalo, não o diff.** 3200 µs por tick até dobrar a chave
   de nome na origem (→ 75 µs) e parar de reordenar coluna que não pode mudar
   (→ zero). O diff nunca foi o problema.
3. **As metas de memória estavam erradas** — pessimistas, e pelo motivo errado.
   O custo por linha é ~1,5 KB; os ~89 MB do femtovg eram piso fixo do
   renderizador.
4. **O domínio não modelava falha.** Escrever o adaptador do librqbit exigiu um
   `match` total e revelou que `State` não tinha `Error` e `TorrentRow` não tinha
   onde guardar a mensagem — o adaptador estaria descartando justamente a metade
   útil de uma falha. Corrigido no core, com a coluna de estado mostrando a
   mensagem em vez da palavra "Error".
5. **O alvo de CPU do download era inventado.** Pedia ≤ 15 % de um núcleo a
   1 Gbps; o medido é ~1,45 % por MB/s, linear, e não se move nem trocando o
   backend de SHA-1 nem limitando conexões. O alvo é que estava errado. Em troca
   apareceu a métrica que faltava e que importa mais: **semeando custa 0,77 % de
   um núcleo**, e é nesse estado que um cliente passa quase toda a vida.

---

## Fase 1 — Esqueleto ✅

**Entregue.** Saiu mais do que o previsto: em vez de uma janela vazia, o
pipeline inteiro de ponta a ponta — tabela, ordenação, seleção, comandos e
atualização otimista — com a engine sintética no lugar do librqbit. Trocar o
corpo dela na Fase 2 não muda nada acima.

| | Alvo | Medido |
|---|---|---|
| Binário | ≤ 12 MB | **8,62 MB** |
| Working set, 2000 torrents | ≤ 60 MB | **31,9 MB** |
| CPU em repouso | ≤ 0,5 % | **0,016 %** |
| Custo do tick | ≤ 1 ms | **70 – 176 µs** |

40 testes, clippy pedantic + nursery limpo, `cargo fmt` em CI nos dois sistemas.

**Dois defeitos que só apareceram rodando:**

1. **Dois timers de 1 Hz entram em fase.** A UI amostrava a engine na mesma
   cadência em que ela publicava, e um snapshot publicado logo depois da
   amostragem esperava um segundo inteiro — medido como um intervalo de 2 s
   entre sequências consecutivas. A UI agora amostra a 250 ms; as três amostras
   que não acham nada custam um load atômico.
2. **O quadro de arranque era pintado duas vezes**, porque o `main` pintava e o
   tick então via uma sequência que ainda não tinha registrado. A primeira
   pintura passou para dentro do tick, que é quem tem o relógio.

**Fora do escopo da Fase 1, e por quê:** bandeja e instância única esperam a
Fase 2, quando há `magnet:` e `.torrent` para entregar a uma instância viva —
antes disso seriam um ícone que não faz nada. `DetailState` e `Prefs` são dois
globals vazios até existir painel que os leia.

<details>
<summary>O que a fase definia</summary>

- Workspace de três crates com a **regra de dependência** valendo desde o commit
  1: `zerem-core` (puro) ← `zerem-engine` (tokio) ← `zerem` (Slint).
- `src/bridge/` com um módulo por domínio e um `wire()` cada, no molde do Vayou.
  **`main.rs` nunca passa de 250 linhas** — é regra, não meta.
- `src/services/` com a lógica pura e testável.
- `ui/state.slint` com **quatro globals** (`SessionState`, `TorrentList`,
  `DetailState`, `Prefs`), não um só — o `PlayerState` do Vayou chegou a ~90
  propriedades num global e é o erro a não repetir.
- `ui/theme.slint` com tokens M3 semânticos. Nenhum hex solto em componente.
- `zerem-shell/` dentro do workspace: updater minisign, single-instance,
  bandeja, componentes Slint. **API sem nada específico de Zerem**, para que
  promovê-lo a repositório compartilhado com Clipo e Vayou depois seja um
  `git mv`, não um refactor.
- CI (`.github/workflows/ci.yml`): `cargo clippy --all-targets` pedantic +
  nursery, `cargo test`, `cargo fmt --check`. Lista de `allow` **curta** — a do
  Clipo, não a do Vayou — e cada `allow` novo vem com o porquê no comentário.
- `dev.bat` no padrão dos outros dois repos, com modos para a comparação de
  renderizadores que a Fase 0 deixou em aberto.
- `[profile.dev.package."*"] opt-level = 2` — **não negociável**: engine torrent
  compilada em debug não satura nem 50 Mbps e mascara toda medição.

</details>

---

## Fase 2 — MVP funcional

**Objetivo:** o app substitui o qBittorrent no uso diário de quem baixa e semeia.
Feio ainda é aceitável aqui; incorreto não.

### Dentro do escopo

**Adicionar** — ✅ menos o arrastar
- ✅ Magnet colado com `Ctrl+V`, e pela linha de comando
- ✅ Arquivo `.torrent` por diálogo nativo (`Ctrl+O`), fora da thread da UI
- ⏸ **Arrastar** — o Slint 1.17 não expõe drop nativamente; alcançá-lo exige a
  escotilha `unstable-winit-030` do Vayou, que fixa a versão menor do Slint.
  Fica para quando houver motivo para pagar esse preço.
- ⏳ Diálogo de adição: nome, tamanho, pasta de destino, **árvore de arquivos com
  seleção** antes de começar

**Lista** — colunas: nome, tamanho, estado, ↓, ↑, peers, ETA, proporção.
Ordenável, redimensionável, seleção múltipla.

O nome vem precedido do **ícone do que o torrent guarda** (vídeo, áudio, imagem,
arquivo, disco, documento ou misto), classificado por peso em bytes a partir da
própria metadata — sem rede, sem adivinhar título, sem terceiro. Uma lista de
nomes se lê; uma lista de formas se varre.

Tamanho e progresso ocupam **uma coluna só**: `491 MB / 1.92 GB · 26%` sobre a
barra que mede isso, e apenas o tamanho depois de completo. A coluna ordena por
tamanho — é a chave que alguém de fato pede, e a única das duas que não força
reordenação a cada tique.

**Ação na linha** — ✅ passar o cursor troca o ícone de tipo por iniciar/pausar,
que age só na linha sob o ponteiro e não mexe na seleção. Não custa coluna nem
desloca a linha em um pixel.

**Teclado e menu** — ✅ setas andam pela lista com `Shift` estendendo, `Home`/`End`
nas pontas, e a lista rola para manter a linha visível. Botão direito abre menu
na linha com iniciar/pausar, abrir pasta, copiar magnet e remover.

**Controle** — ✅ iniciar, pausar, remover com confirmação e "apagar dados" indo
para a **lixeira** (nunca `remove_file`, e os caminhos vêm da metadata do
torrent em vez de serem inferidos da pasta de saída, com guarda contra um
`relative_filename` que sobe de diretório). ✅ abrir pasta e copiar
magnet, pelo menu de contexto. ⏳ forçar reanunciar — o librqbit não expõe.

**Detalhes do selecionado** — ✅ painel **à direita** com abas **Arquivos** e **Peers**,
fechada por padrão. A regra da arquitetura está de fato implementada, não só
documentada: fechar manda `WatchDetails(None)` e a engine **para de montar** as
listas. Não é escondido, é não feito.

A coluna de transporte na aba de peers é o único lugar onde o uTP fica visível,
e era a pergunta em aberto do spike da engine.

✅ **"Não baixar" por arquivo**, na própria aba de Arquivos: cada linha tem um
tique, clicar em qualquer ponto dela alterna, e a linha acima da lista diz o que
sobrou — `3 de 12 arquivos · 1,44 GB de 3,72 GB`. É `update_only_files` do
librqbit, aplicado com a sessão rodando.

O comando **nomeia o arquivo e a resposta, nunca a seleção inteira**: a engine lê
o que está valendo e aplica a mudança àquilo, então um clique dado contra um
painel de um segundo atrás não desfaz o que aconteceu no meio. E a mudança que
deixaria o torrent sem nada para baixar é **recusada** — a mesma regra que o
diálogo de adição já aplica desabilitando o botão. Por isso não há "Nenhum" ao
lado de "Todos": seria um botão cujo único desfecho possível é a recusa.

✅ **Baixar um arquivo antes dos outros**, pela seta na linha. Enquanto ele está
na frente é a única coisa vindo, e a linha acima da lista diz
`1 file first · 11 waiting`; quando chega, os outros voltam sozinhos no tique
seguinte.

Não são os quatro níveis do qBittorrent, e não dá para serem: o librqbit tem uma
alavanca só, `only_files` — a ordenação interna dele é `pub(crate)`, por nome de
arquivo, com um `// TODO: make it configurable` em cima. Um controle de quatro
posições em que três fazem a mesma coisa mentiria sobre o que a engine faz. Foi
construído com a alavanca que existe, e para o caso em que alguém de fato pede
prioridade é o negócio melhor: a banda inteira vai para o arquivo pedido.

Estreitar é seguro, verificado antes de escrever a feature: o
`update_only_files` troca quais peças estão *selecionadas* e não toca em quais
estão *baixadas*. A marca não sobrevive a reiniciar, de propósito; a seleção que
ela estreitou sobrevive, anotada em `narrowed.txt` antes de estreitar e apagada
depois de devolver.

- ⏸ **Trackers** — o librqbit só expõe as URLs (`ManagedTorrentShared::trackers`),
  sem estado de anúncio, seeds ou leechers. Uma aba que lista URLs e nada mais
  não se sustenta; volta quando houver o que mostrar.

**Sessão** — ✅ a lista sobrevive a fechar e reabrir, **com a resume data do
próprio librqbit** em vez de um `session.json` nosso. `Session::new_with_opts`
re-adiciona cada torrent persistido antes de retornar, preservando os ids, então
nada acima da engine sabe que houve um reinício. Com `fastresume` ligado —
desligado por padrão, e desligado significa revalidar todo torrent completo a
cada abertura. Verificado: reiniciar sem argumento nenhum volta com
`restored=1` e `have 336 MiB`, sem revalidação.

O estado vive em `%LOCALAPPDATA%\Zerem`, **não** no `…\rqbit\` que é o padrão do
librqbit — senão uma instalação real do rqbit compartilharia a sessão conosco.

**Preferências** — ✅ pasta de destino, limites globais de ↓/↑ e tema, num
`settings.json` escrito atomicamente (tmp + `sync_all` + rename) e com debounce
de 400 ms. Porta, uTP e UPnP estão no arquivo mas **não no painel**: são fixados
ao construir a sessão, e um controle que silenciosamente não faz nada até o
próximo lançamento é pior que controle nenhum.

Os limites são menu de presets, não campo de texto — ninguém quer digitar "512",
e escolher de uma lista curta é mais rápido e impossível de errar. Aplicados na
hora: os limitadores do librqbit aceitam mudança com a sessão rodando.

Ordenação e larguras de coluna também sobrevivem, e são salvas **enquanto mudam**
e não só na saída — fechar esconde na bandeja, então a saída limpa pode nunca
acontecer.

⏳ Pasta temporária com move-ao-completar, máximo de conexões e idioma.

**Shell**
- ✅ **Instância única** por named pipe, em [`zerem-shell`](crates/zerem-shell/).
  Uma segunda cópia entrega seu argumento à viva e sai — medido em ~1 s, código
  0, um processo só. Se a dona não responder em 5 s ela é considerada travada,
  terminada, e o nome tomado: sem essa sonda um processo pendurado deixa o app
  inabrível, que foi um bug que o Clipo já pagou.
- ✅ **Bandeja** com menu (Mostrar / Sair) e clique esquerdo alternando a janela.
  **Fechar esconde, não encerra** — um cliente que para de semear porque a
  janela incomodava está fazendo a coisa errada.
- ⏳ Handler `magnet:` e associação `.torrent` — o código de registro só faz
  sentido apontando para um caminho instalado, então vai junto com o instalador
  da Fase 4. Registrar de um build em `target/release` sequestraria o handler do
  sistema para um binário que sai do lugar.

> **Foi assim que a instância única subiu de prioridade.** Testando a
> persistência abri duas cópias por engano e as duas abriram a *mesma* pasta de
> sessão e o *mesmo* ISO ao mesmo tempo. A checagem de porta avisava, mas avisar
> não é impedir. Agora está impedido.

### Fora do escopo do MVP

RSS · busca integrada · sequential download e streaming · categorias e tags ·
agendador · filtro de IP · Web UI e modo headless · controle remoto · criação de
torrent · proxy · limites por torrent · plugins. Tudo isso é Fase 5+.

---

## Fase 3 — UX 10/10

**Objetivo:** a fase em que o app deixa de ser "mais um cliente" — e a única que
não pode ser adiada, porque UX enxertada depois nunca fica boa.

✅ **Resposta imediata.** Todo comando aplica **otimista** na UI e o engine confirma
no tick seguinte. Pausar nunca espera a rede.

**Estados desenhados.** ✅ Lista vazia mostra as duas formas de adicionar, não a
frase "nenhum torrent", e um filtro que não casa com nada diz isso e oferece
limpá-lo — não repete o convite de colar magnet para quem já tem trezentos.

✅ **Torrent parado diz por quê**, na própria coluna de estado e no lugar da
palavra "Downloading" — que sobre uma barra que não enche não significa nada.
Quatro respostas, e nenhuma delas é um palpite: `Fetching metadata` para um
magnet cuja lista de arquivos ainda não voltou do enxame, `No peers found` para
quem nunca achou ninguém, `Connecting` para quem achou e não conectou, e
`No one is sharing` para quem conectou e não recebe nada.

As três últimas só aparecem depois de **dez segundos parado**. Peers vão e vêm e
uma peça demora para cair; anunciar falha aos dois segundos é o que ensina a
pessoa a parar de ler a coluna. `Fetching metadata` é imediato: é a diferença
entre "está trabalhando" e "quebrou", e é o que alguém quer saber no segundo
seguinte a colar o link.

Uma **falha** vale mais que uma parada e continua ganhando a coluna: a parada é
o sintoma, o erro é a causa. E as duas que são falha de fato — `No peers found`
e `No one is sharing` — repintam a linha de laranja; `Fetching metadata` e
`Connecting` não, porque é o que um torrent saudável faz nos primeiros segundos
e pintar isso de alerta ensina a ignorar a cor no terceiro torrent.

⏸ **O roadmap pedia mais do que dá para dizer.** "Tracker fora do ar" e "porta
fechada" não são visíveis daqui: o librqbit não reporta estado de anúncio nem
alcançabilidade da porta — a mesma lacuna que já tinha cortado a aba de
Trackers. O que está escrito é exatamente o que as contagens de peers sustentam.
Um palpite vestido de diagnóstico é pior que a palavra "Downloading".

**Erros legíveis e recuperáveis.** ✅ O diálogo de adição pergunta ao volume de
destino quanto cabe e avisa **antes de escrever qualquer coisa** —
`Not enough room in this folder — 2.51 GB short`, logo abaixo da pasta a que se
refere, com o botão **Change** ainda na tela. Prevenir vale mais que traduzir:
um download que morre aos 94 % é recuperável no papel e miserável na prática —
o tempo foi embora, o arquivo parcial ficou, e nada avisou.

O número é o que falta, e não os dois totais: o que o torrent precisa já está na
linha de resumo abaixo, e a quantidade a liberar é a única acionável. É **aviso,
não recusa** — o volume pode ser liberado antes de o download chegar lá, e
arquivos já no disco de uma tentativa anterior são contados como se tivessem de
vir de novo. Bloquear com base numa estimativa que erra a favor do usuário é
pior que dizer qual é a estimativa. E quando o espaço livre não pode ser lido o
app se cala: um aviso construído sobre um desconhecido é o que ensina a fechar
o aviso de verdade sem ler.

O `GetDiskFreeSpaceExW` mora no [`zerem-shell`](crates/zerem-shell/), neutro como
o resto dele, e **não custou dependência nova** — o `windows` já estava lá com a
feature certa. No Linux responde `None`, porque `statvfs` significa `libc` e essa
dependência entra com o build Linux, não antes.

✅ **E a metade reativa.** Um torrent que falha mostra a mensagem no lugar da
palavra "Error" — e agora **em palavras, não em número**. O que chega do engine
termina em `(os error 112)`, que é um fato sobre o kernel e não sobre o
download; os códigos que valem a pena viram frase: sem espaço, sem permissão,
pasta sumiu, arquivo aberto por outro programa, pasta somente leitura, unidade
indisponível.

Os códigos são por plataforma de propósito — `ENOSPC` é 28 e `ERROR_DISK_FULL`
é 112, a mesma falha com dois números. Casar pelo texto seria pior: o Windows
traduz as mensagens dele, então funcionaria numa instalação em inglês e pararia
de funcionar numa em português. O que não é reconhecido passa intacto: uma
tradução errada é pior que um código cru, que pelo menos dá para pesquisar.

A ação fica no painel, acima das abas: a mensagem quebrando linha — a coluna de
estado já tentou reticenciar, e é para isso que o painel serve — e **Try again**
ao lado. É retentativa de verdade: dar start num torrent em erro faz o librqbit
re-inicializar, conferir o que está no disco e seguir dali. Verificado no código
dele, não suposto.

⏸ **"Escolher outra pasta" não entra**, e não é esquecimento: o librqbit fixa a
pasta de saída no momento em que o torrent é adicionado e não expõe como movê-la
depois. O que o app pode fazer sobre espaço em disco ele faz antes, no diálogo
de adição.

✅ **Números estáveis.** A taxa que o librqbit informa já é uma média de cinco
segundos; o que sobra de ruído é a janela dela, um segundo entrando e um saindo.
Então o instrumento é uma **zona morta** e não mais média — empilhar um segundo
alisamento pesado acrescentaria atraso a um número que já está cinco segundos
atrás. Começar e parar não são filtrados: parar publica zero na hora, começar
publica a primeira amostra inteira. O ETA vem da taxa publicada, não da crua.

A zona morta paga duas vezes: uma taxa que não muda deixa a linha idêntica byte
a byte entre dois tiques, e o diff do modelo pula a linha inteira. **Suavizar
sai mais barato que não suavizar.**

**Teclado completo.** ✅ Setas, `Space` pausa, `Del` remove, `Enter` abre a pasta,
`Ctrl+V` cola magnet, `Ctrl+F` vai para o filtro e `Esc` volta para a lista.
⏳ Rebindável no molde do `keybindings.rs` do Vayou.

✅ **Movimento com propósito.** Dois tokens de duração e duas curvas no
[`theme.slint`](ui/theme.slint), e nada mais: o M3 publica uma dúzia de cada, e
um token que ninguém gasta é um token que se afasta do que o app faz. As curvas
são as do M3 — `standard` para algo mudando no lugar, `emphasized` para algo
chegando ou saindo.

O painel de detalhes desliza, 240 ms na `emphasized`. A folha é desenhada uma
vez na largura cheia e passa por baixo do recorte; animar a largura do layout
faria cada caminho reticenciar de novo a cada quadro, e texto pulando entre duas
elisões não é movimento, é defeito. A animação vive num `reveal` de 0 a 1 e não
na largura, para que arrastar a borda continue colado no cursor.

As animações que já existiam soltas — 90 ms `ease-out` copiadas em cinco
lugares — passaram a ler os mesmos tokens.

⏸ **Entrada e saída de linha não entra.** O `ListView` do Slint não tem ciclo de
vida por item onde pendurar isso, e a alternativa é uma animação por linha —
que a 2000 linhas é exatamente o custo que a Fase 0 passou o tempo dela
removendo. Volta se o Slint expuser o gancho.

✅ **Sparkline de 60 s** no rodapé para ↓/↑ agregado, ao lado dos dois totais que
ela desenha. Forma e não escala: sem eixo, porque a pergunta que ela responde é
"estável, subindo ou morrendo", não "quanto". Uma escala vertical para as duas
linhas, então o upload se lê contra o download a olho — e como as amostras são
as taxas já publicadas, a escala herda a estabilidade delas. Minuto ocioso não
desenha nada, e voltar da bandeja recomeça o minuto em vez de emendar dois.

✅ **Verificado na tela**, com o app baixando: a linha azul do download tem
forma e a verde do upload fica no chão, exatamente como desenhado. O `Path` do
renderizador por software desenha traços — é o `zeno` por baixo, e ele já estava
linkado: trocar as duas linhas por um retângulo devolve 10 KB do binário.

A mesma captura pegou um bug que nenhum teste pegaria: **cinco ícones estavam
desenhados errado**. Um `m` minúsculo abrindo um subcaminho concatenado é
relativo a onde o subcaminho anterior terminou, então a seta do rodapé era uma
haste com a ponta fora da grade, o sol não tinha raios diagonais, e a lupa e o
ícone de imagem tinham perdido um traço cada. O comentário no topo do
`icons.slint` afirmava que a concatenação era segura; agora diz quando é.

⏳ **Acessibilidade** — contraste verificado, rótulos AccessKit, navegação por
teclado sem armadilha de foco.

⏳ **i18n** — 12 idiomas via `.po` empacotado, mesmo pipeline do Vayou.

**Encerramento desta fase:** a tabela de alvos de aceite lá em cima é medida e
publicada em `docs/benchmarks.md`. Alvo não atingido vira bug bloqueante.

---

## Fase 4 — Distribuição

- Instalador NSIS **por usuário, sem UAC** (`%LOCALAPPDATA%\Programs\Zerem`).
- Auto-update com **verificação de assinatura minisign** antes de trocar o
  binário, `latest.json` com uma entrada por plataforma, e `install_kind()`
  reconhecendo instalação que o app não pode reescrever — o `update.rs` do Vayou
  copiado inteiro.
- Benchmarks de regressão no CI (criterion sobre o rate limiter, o diff do modelo
  e a ordenação) + checagem de RAM ociosa. Sem isso, "10 em performance" volta a
  ser opinião em três meses.
- `README.md` no padrão visual de Clipo e Vayou.

---

## Fase 5+ — Backlog

Em ordem de valor por custo. Nada entra antes da Fase 4 fechada.

| Prioridade | Item | Nota |
|---|---|---|
| Alta | Limites e prioridade **por torrent** | Falta sentida cedo |
| Alta | Categorias / pastas de destino automáticas | Base para o resto |
| Alta | **RSS** com filtros | O que fixa quem acompanha série |
| Alta | Sequential download + streaming | Diferencial real |
| Média | Modo headless + Web UI | Reaproveita a camada de comandos inteira |
| Média | Busca integrada por plugins | Cuidado com a superfície legal |
| Média | Agendador por horário | Simples, muito pedido |
| Média | Linux (o `win/` já prevê) | O Vayou já pavimentou |
| Baixa | Criação de torrent · filtro de IP · proxy | Cauda longa |

---

## Riscos conhecidos

| Risco | Impacto | Mitigação |
|---|---|---|
| **uTP do librqbit** está atrás de flag e desligado por padrão | Sem ele o upload mata a latência da conexão e alguns peers ficam inalcançáveis | Validado na S0.2, **na primeira semana** — não no fim |
| **Tabela do Slint** sem componente pronto para o que o app precisa | Retrabalho grande se descoberto tarde | S0.1 é a primeira coisa do projeto |
| **Licença do Slint** — GPLv3, royalty-free desktop ou comercial | Um binário distribuído tem de estar coberto por uma delas | Decidir a licença do Zerem **antes** do primeiro release, e documentar como Clipo e Vayou já fazem |
| **Tracker privado** filtra por `peer_id` | Cliente próprio não passa no whitelist | Limitação do projeto, não bug. Documentar no README desde o início |
| Ordenar / filtrar dentro do `.slint` | Mata o frame rate com lista grande | Regra dura: **o Slint não tem lógica**; o modelo chega pronto do Rust |
