<div align="center">

# Zerem

**Cliente BitTorrent nativo. Leve, rápido, e com uma tabela que não engasga.**

Rust + [Slint](https://slint.dev) · um processo · sem WebView · renderização por software

![Rust](https://img.shields.io/badge/Rust-1.98-CE422B)
![Slint](https://img.shields.io/badge/UI-Slint%201.17-2379F4)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0078D6)

[Roadmap](ROADMAP.md) · [Arquitetura](ARCHITECTURE.md) · [Relatório dos spikes](docs/spike-report.md)

</div>

---

> **Estado: usável no dia a dia.** A engine é o librqbit. Colar um magnet, abrir
> um `.torrent`, iniciar, pausar e remover funcionam ponta a ponta; a lista, as
> preferências e o estado da tabela sobrevivem a fechar e reabrir; há bandeja com
> instância única, limites de banda e painel de detalhes. O diálogo de adição
> escolhe os arquivos antes de começar, e a aba de Arquivos os troca depois. O
> filtro responde a cada tecla, o diálogo avisa quando não vai caber no disco,
> um torrent que empaca diz por quê e um arquivo pode furar a fila. **A Fase 3
> fechou** — números estáveis, sparkline, movimento, contraste aferido, leitor de
> tela e a interface em português — e os alvos estão medidos em
> [docs/benchmarks.md](docs/benchmarks.md). O que falta é o segundo catálogo da
> i18n e a Fase 4 inteira: instalador, auto-update e benchmarks de regressão no
> CI. A ordem está no [roadmap](ROADMAP.md).

## Preferências

`Ctrl+,` ou o botão na barra. Pasta de destino, limite de download e limite de
upload — aplicados na hora, sem reiniciar.

O **limite de upload** é o que importa no dia a dia: sem ele o cliente enche o
uplink e trava a navegação da casa inteira. Os limites são um menu de presets em
vez de campo de texto — ninguém quer digitar "512".

Tudo vive em `%LOCALAPPDATA%\Zerem\settings.json`, escrito atomicamente e com
debounce. **Porta de escuta, uTP e UPnP estão no arquivo mas não no painel**:
são fixados quando a sessão é construída, e um controle que silenciosamente não
faz nada até o próximo lançamento é pior que controle nenhum. Editar o arquivo à
mão funciona, inclusive salvo pelo Bloco de Notas — o BOM que ele escreve é
tolerado.

## Medido, não prometido

| | Alvo | Medido — método em [benchmarks.md](docs/benchmarks.md) |
|---|---|---|
| Binário | ≤ 16 MB | **15,37 MB** — o librqbit responde por 4,7 |
| Cold start até a janela | ≤ 300 ms | **92 ms** — mediana de 5 execuções |
| Working set, sessão viva | ≤ 60 MB | **57,6 MB** |
| Working set, 2000 linhas (Fase 1) | ≤ 60 MB | **31,9 MB** |
| CPU baixando | ≤ 1,5 % de um núcleo por MB/s | **1,45 %** |
| CPU semeando | ≤ 1 % de um núcleo | **0,77 %** |
| Custo do tick | ≤ 1 ms | **p95 60 µs, máx 71 µs** |
| Janela escondida | ≤ 0,1 % | **0,000 %** — o tick para, os downloads não |

## Como está montado

```
zerem (bin)        Slint + bridge/ — conhece engine e core
zerem-engine       sessão, tick, contrato Snapshot/Command — nunca conhece Slint
zerem-core         tipos, ordenação, formatação — sem async, sem I/O, sem UI
zerem-shell        encanamento de desktop (instância única) — não conhece o Zerem
```

`zerem-shell` é deliberadamente neutro: nada ali nomeia o Zerem, então promovê-lo
a um repositório compartilhado com Clipo e Vayou é mover a pasta.

A seta de dependência só aponta para baixo. É o que mantém o core testável sem
runtime nem janela, e o que vai tornar o modo headless da Fase 5 quase de graça:
é a mesma engine com outro consumidor de `Snapshot`.

**Estado desce, comando sobe.** A engine publica um `Snapshot` imutável por
segundo na thread dela; a UI compara com o que já desenhou e notifica só as
linhas que mudaram. Nenhum clique espera a engine: o comando é aplicado
otimisticamente na hora e o primeiro snapshot seguinte é a verdade — inclusive
quando a engine recusou, e aí a linha volta sozinha.

## Rodar

```bat
dev.bat              o app, debug — com log por tick
dev.bat release      o app, release — o único build que vale medir
dev.bat table        spike da tabela, renderizador por software
dev.bat table gpu    spike da tabela, femtovg — o outro lado da comparação
```

Os dois últimos existem para a verificação da Fase 0 que ainda precisa de olho
humano: rolar a lista de ponta a ponta nos dois renderizadores. A repintura
parcial é o que torna o de software barato, e ela não ajuda quando a viewport
inteira se move.

Ou direto:

```powershell
cargo run --release
$env:ZEREM_LOG = "debug"   # imprime o custo de cada tick
```

## Idioma

`Ctrl+,` → **Idioma**. Começa em **System**, que segue o que a máquina estiver
configurada — guardado como vazio e não como uma etiqueta, para que trocar o
idioma do Windows troque o do app em vez de deixá-lo preso ao que era no dia da
primeira abertura. A troca é **ao vivo**: o Slint guarda a seleção como
propriedade, então a janela inteira redesenha sem reiniciar.

`pt-PT` cai em `pt-BR` de propósito. Uma máquina em português está pedindo
português, e devolver inglês seria a resposta errada para uma pergunta que foi
quase respondida.

**Os `.po` são compilados dentro do binário**, não carregados. A outra opção do
Slint é gettext, e no Windows isso significa compilar a biblioteca GNU com
autotools sob MSVC — uma toolchain que ninguém deveria precisar para rodar
`cargo build`. Empacotar custa uma pasta de `.po` e nenhuma dependência de
runtime.

**O que está traduzido, e o que não está.** As 56 strings do `.slint` — botões,
diálogos, estados vazios, rótulos de acessibilidade — estão. As ~54 que o Rust
monta — os estados da coluna, as causas de parada, as frases de falha, as
unidades — **ainda não**: elas vivem em Rust porque a regra do projeto é que o
`.slint` nunca formata nada, e o `@tr()` com argumentos é avaliado por quadro
dentro de um `for`, que é exatamente o custo que essa regra existe para evitar.
Elas precisam de um segundo catálogo, e é o próximo passo.

**Só pt-BR por enquanto.** Traduzir 140 strings para 12 idiomas sem ninguém para
revisar seria publicar 11 conjuntos de erros plausíveis. A mecânica está pronta:
um idioma novo é uma pasta em `lang/` e uma linha em
[`language.rs`](crates/zerem-core/src/language.rs) — não há terceiro passo, e
esquecer a linha é o que um teste pega.

Dois testes guardam isso, e ambos leem os arquivos de verdade em vez de uma
cópia: [`tests/translations.rs`](tests/translations.rs) compara cada `@tr()`
com cada `msgid` **nos dois sentidos** — o Slint deixa passar uma tradução
faltando em silêncio, desenhando o original, que é o comportamento certo em
runtime e o errado de se descobrir por captura de tela.

## Teclado

| Tecla | Ação |
|---|---|
| `Ctrl+V` | cola um magnet da área de transferência |
| `Ctrl+O` | abre um `.torrent` |
| `Ctrl+,` | preferências |
| `Ctrl+F` | vai para o filtro; `Esc` volta para a lista |
| `Ctrl+I` | abre ou fecha o painel de detalhes — clicar numa linha também abre |
| `Espaço` | pausa ou inicia a seleção |
| `Enter` | abre a pasta da seleção |
| `Delete` | remove a seleção — **pergunta antes** |
| `↑` `↓` | anda pela lista; com `Shift` estende a seleção |
| `Home` `End` | topo e fim |
| `T` | alterna tema |

**Botão direito** abre um menu na linha: iniciar/pausar, abrir pasta, copiar
magnet, remover. Clicar com o direito numa linha não selecionada seleciona ela
primeiro — um menu que age em outra coisa que não a clicada é a forma clássica
de apagar o errado.

Clique no cabeçalho ordena, clique de novo inverte. A alça na borda direita
redimensiona a coluna. `Ctrl` alterna a seleção, `Shift` seleciona intervalo.

**Passar o cursor** sobre uma linha troca o ícone de tipo por iniciar/pausar.
Age só naquela linha e não mexe na seleção — é o caminho curto para a ação mais
comum, sem selecionar antes nem subir até a barra.
Um `.torrent` ou magnet passado na linha de comando é aberto no arranque.

**Quando um torrent falha**, a coluna de estado diz o quê em palavras e não em
número: o que chega do engine termina em `(os error 112)`, que é um fato sobre o
kernel e não sobre o download. Sem espaço, sem permissão, pasta sumiu, arquivo
aberto por outro programa — os códigos que valem viram frase, e o que não é
reconhecido passa intacto, porque tradução errada é pior que código cru.

O painel mostra a mensagem inteira acima das abas, com **Try again** ao lado.
É retentativa de verdade: dar start num torrent em erro faz o librqbit
re-inicializar, conferir o que está no disco e seguir dali.

**Remover é a única coisa destrutiva que o app faz, e a única que pergunta.**
A caixa "mover os dados para a lixeira" começa desmarcada toda vez — a escolha
recuperável tem de ser a fácil. Quando marcada, os arquivos vão para a lixeira,
nunca para um `remove_file`.

> Arrastar arquivos ainda não funciona: o Slint 1.17 não expõe drop nativamente,
> e alcançá-lo exige a escotilha `unstable-winit-030` que o Vayou usa. Fica para
> quando houver motivo para fixar a versão menor do Slint.

## Detalhes

**Clicar numa linha abre** um painel **à direita** da tabela, com abas de
**Arquivos** e **Peers** daquele torrent — do jeito que um cliente de e-mail
abre a mensagem em que se clica. Procurar um botão para ver o que tem dentro de
um torrent é um passo que ninguém deveria ter de achar. `Ctrl+I` e o botão na
barra continuam alternando, e o × do painel fecha.

Só o clique simples abre. `Ctrl` e `Shift` estão montando uma seleção de
vários, e um painel só mostra um: abri-lo ali seria escolher um torrent do
grupo no lugar de quem clicou.

À direita e não embaixo: um painel sob a tabela custa linhas, que é a única
coisa para que a janela serve; um painel ao lado custa colunas, das quais as
últimas são as menos urgentes. É também o que o M3 pede — *bottom sheet* é o
padrão móvel, *side sheet* o de desktop. Em compensação, com o painel aberto
numa janela estreita as últimas colunas ficam cortadas; estreite uma coluna ou
alargue a janela.

**360 px**, que é a medida do *side sheet* do M3 — 256 e 400 são o mínimo e o
máximo que a spec dá a ele. Tomada emprestada em vez de inventada: um painel ao
lado de uma lista não tem proporção própria de onde sair, e derivar uma da
janela é binding loop — o Slint fala em voz alta quando se tenta. A fonte
honesta é o sistema de design que o app já segue em todo o resto.

**A borda esquerda arrasta**, com a mesma alça e o mesmo cursor das colunas da
tabela — é o mesmo gesto. A largura sobrevive a fechar e reabrir, como as
colunas: ninguém escolhe essa medida num painel de preferências, se chega nela;
chegar nela duas vezes é que é o aborrecimento.

**Ele desliza para dentro**, 240 ms na curva *emphasized* do M3. A folha é
desenhada uma vez na largura cheia e passa por baixo do recorte; animar a
largura do layout faria cada caminho e cada endereço reticenciar de novo a cada
quadro, e texto pulando entre duas elisões não é movimento, é defeito.

Cada linha do painel tem duas: o caminho ou o endereço, e os números embaixo.
Isso lê melhor em 360 px do que uma tabela de quatro colunas jamais leu em 1180.

**O tique na aba de Arquivos liga e desliga o download de cada um**, com o
torrent rodando. Clicar em qualquer ponto da linha alterna — um quadrado de
16 px é pouco para se acertar cinquenta vezes seguidas — e a linha acima da
lista diz o que sobrou: `3 de 12 arquivos · 1,44 GB de 3,72 GB`. Um arquivo
desligado esmaece em vez de sumir: ele continua no torrent, só não está vindo.

Deixar o torrent **sem nenhum arquivo é recusado**, com a explicação na barra de
status. É por isso que só existe "Todos" e não "Nenhum": escolher três entre
quatro mil é o que o diálogo de adição faz, e aqui o botão serve para voltar de
ter desmarcado demais.

Fechar o painel **para a engine de montar** essas listas, não só de desenhá-las.
Com quinhentos torrents, montar a tabela de peers de todos uma vez por segundo é
como um cliente queima um núcleo sem mover um byte.

A coluna de transporte é o único lugar onde o uTP aparece. Trackers ainda não
têm aba: o librqbit expõe as URLs e nada mais — sem estado de anúncio, seeds ou
leechers — e uma aba que lista URLs não se sustenta.

**Baixar um arquivo antes dos outros** — o tique liga e desliga; a **seta ao lado**
manda o arquivo para a frente. Aparece ao passar o cursor na linha, e fica acesa
depois de marcada, porque ela é o motivo de o resto do torrent ter parado.

E parou mesmo: **enquanto um arquivo está na frente, ele é a única coisa
vindo**. A linha acima da lista troca a contagem por `1 file first · 11 waiting`
— o número que importa não é quantos foram priorizados, é quantos pararam para
isso. Quando o arquivo chega, os outros voltam sozinhos, no tique seguinte, sem
ninguém apertar nada.

**Não são os quatro níveis do qBittorrent, e não dá para serem.** O librqbit tem
uma alavanca só, `only_files`; a ordenação interna dele é `pub(crate)`, por nome
de arquivo, com um `// TODO: make it configurable` em cima. Um controle de
quatro posições em que três fazem a mesma coisa mentiria sobre o que a engine
faz. Então "primeiro" foi construído com a alavanca que existe — e para o caso
em que alguém de fato pede prioridade (*quero este episódio agora*) é o negócio
melhor, porque a banda inteira vai para ele em vez de uma fatia.

Estreitar é seguro, e isso foi verificado antes de escrever a feature: o
`update_only_files` do librqbit troca quais peças estão *selecionadas* e não
toca em quais estão *baixadas*, então voltar atrás devolve exatamente o
progresso que estava lá.

Marcar um arquivo que está desligado é recusado — os dois controles não discutem
sobre o mesmo arquivo. Desligar um arquivo marcado tira a marca junto.

**A marca não sobrevive a reiniciar**, de propósito: "baixe este primeiro" é uma
instrução dada num momento, não uma configuração. Voltar dias depois com o
torrent ainda segurando o resto de si mesmo seria o app lembrando a metade
errada. O que sobrevive é a *seleção* que a marca estreitou — anotada em
`%LOCALAPPDATA%\Zerem\narrowed.txt` antes de estreitar e apagada depois de
devolver, para que um processo morto no meio não volte com onze de doze arquivos
desligados e nenhuma explicação.

## Antes de encher o disco

O diálogo de adição pergunta ao volume de destino quanto cabe, e avisa **antes**
de escrever qualquer coisa: `Not enough room in this folder — 2.51 GB short`,
logo abaixo da pasta a que se refere e com o botão **Change** ainda na tela.

O número é o que falta, não os dois totais. O que o torrent precisa já está na
linha de resumo logo abaixo; a quantidade que tem de ser liberada — ou que tem
de sair da lista de tiques — é a única acionável.

**É aviso, não recusa, e as duas razões são honestas.** O volume pode ser
liberado muito antes de o download chegar no fim dele, e arquivos que já estão
no disco de uma tentativa anterior são contados aqui como se tivessem de vir de
novo. Bloquear com base numa estimativa que pode errar a favor do usuário é pior
que dizer qual é a estimativa.

Quando o espaço livre **não pode ser lido**, o app não diz nada. Um aviso
construído sobre um desconhecido é pior que aviso nenhum: é o que ensina a
pessoa a fechar o aviso de verdade sem ler. No Linux ainda é esse o caso — a
resposta é `statvfs`, que significa `libc`, uma dependência que o
[`zerem-shell`](crates/zerem-shell/) não vai adquirir por uma chamada antes de o
build Linux precisar dela.

## Quando nada acontece

A coluna de estado de um torrent que está rodando e não anda **diz por quê**, no
lugar da palavra "Downloading" — que sobre uma barra que não enche não significa
nada, e é o spinner eterno que todo cliente tem.

| O que aparece | O que aconteceu |
|---|---|
| `Fetching metadata` | magnet cuja lista de arquivos ainda não voltou do enxame |
| `No peers found` | nunca achou ninguém |
| `Connecting` | achou peers e não conectou em nenhum |
| `No one is sharing` | conectou, e nada está chegando |

As três últimas esperam **dez segundos parado** antes de aparecer. Peers vão e
vêm e uma peça demora para cair; anunciar falha aos dois segundos é o que ensina
a pessoa a parar de ler a coluna. `Fetching metadata` é imediato — é a diferença
entre "está trabalhando" e "quebrou".

Só as duas que são falha de fato repintam a linha de **laranja**. Buscar
metadata e conectar é o que um torrent saudável faz nos primeiros segundos, e
pintar isso de alerta ensina a ignorar a cor no terceiro torrent.

**O que o app não diz, e por quê.** "Tracker fora do ar" e "porta fechada" não
são visíveis daqui — o librqbit não expõe estado de anúncio nem alcançabilidade,
a mesma lacuna que deixou os Trackers sem aba. O que está escrito é exatamente o
que as contagens de peers sustentam. Um palpite vestido de diagnóstico é pior
que a palavra "Downloading".

## Números que ficam parados

A velocidade que o librqbit informa já é uma média — ele divide os bytes de uma
janela deslizante de cinco segundos. O que sobra de ruído não é amostragem, é a
própria janela, um segundo entrando e um saindo: pequeno, alguns por cento, e
**incessante**. Um megabyte por segundo estável imprime `1,03 MB/s`, `1,00 MB/s`,
`1,05 MB/s`, e um valor sentado em cima de uma fronteira de unidade alterna entre
`999 KB/s` e `1,00 MB/s` dez vezes por minuto. É o detalhe que faz um cliente
parecer amador.

Então o instrumento é uma **zona morta**, não mais média: o número publicado só
se move quando a taxa de fato saiu de perto dele. Empilhar uma segunda média
pesada em cima da do librqbit compraria a mesma quietude acrescentando atraso a
um número que já está cinco segundos atrás.

**Começar e parar não são ruído e não são filtrados.** Parar publica zero na
hora — uma linha pausada mostrando 4 MB/s por três segundos é mentira — e
começar publica a primeira amostra inteira, em vez de subir a partir do nada
enquanto o download já está a todo vapor.

A zona morta paga duas vezes. Uma taxa que não muda deixa a linha idêntica byte
a byte entre dois tiques, então o diff do modelo pula a linha e não a reformata
nem a repinta: **suavizar sai mais barato que não suavizar.**

O ETA vem da taxa publicada, não da crua — um tempo restante calculado sobre um
número e desenhado ao lado de outro são duas colunas se contradizendo.

## O rodapé

Ao lado dos dois totais, **sessenta segundos** de ↓ e ↑ agregados. Forma, não
escala: os números estão impressos do lado, e o que uma linha acrescenta é a
única coisa que eles não dizem — se aquilo está indo a algum lugar. Sem eixo,
pelo mesmo motivo: um rodapé não tem onde rotular um, e a pergunta que ele
responde é "estável, subindo ou morrendo", não "quanto".

As duas linhas dividem uma escala vertical, então o upload se lê contra o
download a olho. O topo da caixa é o segundo mais rápido do minuto — e como as
amostras são as taxas já publicadas, a escala herda a estabilidade delas em vez
de pular a cada tique.

Um minuto ocioso **não desenha nada**. Uma linha grudada no fundo da caixa diz
menos do que o espaço que ela ocupa diria vazio.

Esconder a janela **para o tique**, então nada é amostrado enquanto ela está
fora. Voltar recomeça o minuto em vez de emendar dois minutos separados e
chamar o resultado de "os últimos sessenta segundos".

## Bandeja

**Fechar a janela esconde, não encerra.** Um cliente que para de semear porque a
janela incomodava está fazendo a coisa errada. Sair é pelo menu da bandeja.

Isso só é aceitável por causa do trabalho já feito: com a janela escondida o
tick da UI **para** — a Fase 0 mediu esse estado em 0,000 % de CPU — enquanto a
engine continua transferindo. Clique esquerdo no ícone alterna a janela.

Abrir o Zerem de novo enquanto ele já roda **não abre uma segunda cópia**: a
nova entrega o que recebeu à instância viva e sai. Sem isso, duas cópias abririam
a mesma pasta de sessão e o mesmo arquivo ao mesmo tempo.

## Gate de qualidade

Tudo isto tem de passar antes de qualquer merge — é o que o
[CI](.github/workflows/ci.yml) roda, no Windows e no Linux:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets    # pedantic + nursery, -D warnings
cargo test --workspace
```

O compilador é fixado em [`rust-toolchain.toml`](rust-toolchain.toml): sem isso,
cada versão nova do Rust deixa o CI vermelho em código que ninguém tocou.

## Componentes de terceiros

| Componente | Termos |
|---|---|
| **Slint** (toolkit de UI) | Licenciamento próprio — GPLv3, royalty-free desktop, ou comercial. Um binário distribuído tem de estar coberto por uma delas; ver [slint.dev](https://slint.dev). **A escolha para o Zerem é decisão da Fase 4, antes do primeiro release.** |
| **librqbit** (engine BitTorrent) | Apache-2.0. Fonte em [github.com/ikatson/rqbit](https://github.com/ikatson/rqbit). |

## Limitação conhecida

**Trackers privados filtram por `peer_id`**, e um cliente próprio não passa no
whitelist deles. Isso é uma limitação do projeto, não um bug — se você usa
tracker privado, o Zerem não substitui o seu cliente atual.

## Licença

MIT © Ohgawa
