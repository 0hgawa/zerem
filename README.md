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
> um torrent que empaca diz por quê e um arquivo pode furar a fila. Há fila com
> máximo de ativos, categorias com pasta automática, bandeira do país ao lado de
> cada peer, menu de botão direito nos arquivos e **mover ao completar**. **A
> Fase 3 fechou** — números estáveis, sparkline, movimento, contraste aferido,
> leitor de tela e a interface inteira em português, dos dois lados: o `.slint`
> e o Rust, cada um com teste nas duas direções. Os alvos estão medidos em
> [docs/benchmarks.md](docs/benchmarks.md), agora com criterion por trás.
>
> **O que falta é a Fase 4**: instalador NSIS (o script existe, falta compilar),
> registro de `magnet:` e `.torrent` — que depende do instalador — e auto-update
> com minisign. E **a licença do Slint** precisa ser escolhida antes de qualquer
> release: GPLv3, royalty-free desktop ou comercial. A ordem está no
> [roadmap](ROADMAP.md).

## Preferências

`Ctrl+,` ou o botão na barra. Cinco salas — **Geral**, **Downloads**,
**Velocidade**, **Conexão** e **Sobre** — num trilho à esquerda, com o painel à
direita. Era um scroll só com títulos dentro, o que fazia o caminho até a porta
de escuta passar por cima dos limites de banda.

Não há botão *Pronto*. Cada mudança é escrita no instante em que acontece, então
não existe nada pendente que um botão pudesse concluir — só o × no canto, onde
toda janela deste desktop guarda o dela.

Dentro de uma sala as linhas ficam em cards, e cada linha diz o que é à esquerda
e carrega o que a muda à direita. É a forma de toda tela de configuração deste
desktop, e substituiu uma coluna fixa de rótulo que fazia cada linha parecer
campo de formulário em vez de escolha.

Os números são **digitados**, não escolhidos numa lista. Os presets que vieram
antes eram mais rápidos de trocar e impossíveis de errar, e esse argumento só
vale enquanto todo número que alguém quer está na lista — o que um limite de
banda nunca é: as pessoas têm uma velocidade de linha e querem um número que se
relacione com ela. Vazio significa sem limite, que é o que o texto de fundo diz,
em vez de uma palavra para apagar antes de digitar.

O valor entra ao apertar Enter ou ao sair do campo, **nunca a cada tecla**:
digitar "500" caractere a caractere aplicaria 5, depois 50, e os dois primeiros
estrangulam a conexão por um instante.

O **limite de upload** é o que importa no dia a dia: sem ele o cliente enche o
uplink e trava a navegação da casa inteira.

Há **dois pares de limites**, e o botão do raio na barra alterna entre eles. Dois
pares em vez de um que se edita: o objetivo é ficar quieto por uma noite e
voltar, e um par só significa redigitar os números de verdade de memória toda
vez. E fica a um clique de distância de propósito — o momento em que alguém quer
a linha de volta não é um momento em que essa pessoa quer abrir um painel.

**Porta de escuta, uTP, UPnP e criptografia estão no painel** e dizem, uma vez
para todos, que valem a partir do próximo lançamento. Isso reverte uma decisão anterior de
escondê-los: a objeção estava certa — um controle que *silenciosamente* não faz
nada até reabrir é pior que controle nenhum — e ela era um argumento contra o
silêncio, não contra o controle.

Tudo vive em `%LOCALAPPDATA%\Zerem\settings.json`, escrito atomicamente e com
debounce. Editar o arquivo à mão funciona, inclusive salvo pelo Bloco de Notas —
o BOM que ele escreve é tolerado.

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

**São dois catálogos, e não é duplicação.** Os conjuntos são disjuntos: um
segura a moldura, o outro as frases feitas de números. As 56 strings do `.slint`
vivem nos `.po`; as que o Rust monta — estados da coluna, causas de parada,
frases de falha, as contagens do painel — vivem em
[`text.rs`](crates/zerem-core/src/text.rs), porque a regra do projeto é que o
`.slint` nunca formata nada e `@tr()` com argumentos é avaliado por quadro
dentro de um `for` — exatamente o custo que essa regra evita.

Lá dentro há duas formas, e a divisão não é estilística. String fixa entra numa
tabela e é buscada pelo próprio original em inglês, como o gettext faz. String
com número dentro não pode: `format!` exige literal, então essas são escritas
uma por idioma, onde a ordem das palavras é livre para diferir e o compilador
ainda confere os argumentos. É por isso que "faltam 2,51 GB" pôde virar frase de
gente em vez de um template com as palavras embaralhadas em volta do buraco.

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
| `Ctrl+A` | seleciona tudo o que a lista está mostrando |
| `Ctrl+I` | abre ou fecha o painel de detalhes — clicar numa linha também abre |
| `Espaço` | pausa ou inicia a seleção |
| `Enter` | abre a pasta da seleção — duplo clique também |
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

**Cada linha tem os próprios botões, sempre visíveis.** Iniciar/pausar **na
frente**, antes do nome; remover **no fim**, do outro lado da linha.

Essa separação é o ponto, não uma inconsistência: **o que se faz o tempo todo
fica onde a mão está, e o que não dá para desfazer fica o mais longe dela que a
linha permite**. Na frente, o botão de pausar fica num x fixo — dá para pausar
uma sequência de linhas sem a mão andar de lado — e é onde o olho já está,
porque o olho está nos nomes. E os dois deixaram de ser vizinhos, então o
destrutivo nunca é o erro de mira do outro.

Os dois agem só na linha sob o cursor e não mexem na seleção: sem selecionar
antes, e sem chance de acertar um torrent que o cursor não está em cima.
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

O botão da barra que alternava esse painel foi removido, e o `Ctrl+I` ficou. Ele
não é uma visão da janela, é uma visão de **um torrent**: um interruptor global
para algo sempre *sobre uma linha selecionada* é um interruptor que não faz nada
em metade das vezes que é apertado.

### Botão direito num arquivo

**Abrir**, **abrir a pasta do arquivo**, e as três prioridades que existem. O
duplo clique faz o mesmo que a primeira linha.

O qBittorrent oferece prioridade em quatro níveis. Três deles não existem neste
motor: o librqbit sabe se um arquivo é desejado e se **um** arquivo está sendo
buscado à frente dos outros, e nada entre isso. Oferecer "Alta" e "Máxima" como
linhas separadas seriam dois controles fazendo a mesma coisa — pior que um menu
mais curto.

**Abrir** fica desligado até o arquivo terminar; um vídeo pela metade abre como
alguns segundos e um erro de codec. No menu ele apaga; no duplo clique, que não
tem estado apagado para mostrar, a barra de status diz por quê.

### Bandeira do país

Ao lado do endereço de cada peer, onde é lido primeiro e responde "quem é este?"
antes de o número ter de responder.

A tabela vem dos arquivos de delegação dos cinco RIRs — o registro em si, não um
palpite sobre ele, livre para redistribuir, e sem contrato de licença entre o
Zerem e quem o usa por causa de uma bandeira. São 504 KiB e **nada é
arredondado**: um piso de /22 economizaria 114 KiB e essa economia é feita
inteiramente de erros, porque as únicas faixas que um piso remove são as de país
diferente do vizinho que as engole. O `1.1.1.0/24` saía como Tailândia.

Emoji seria de graça e não funciona: a Segoe UI Emoji não traz os pares de
indicador regional, de propósito, então 🇧🇷 no Windows sai como as letras B e R.
As 239 bandeiras são imagens, empacotadas em 104 KiB com paleta e comprimentos
de corrida.

Espaço que ninguém recebeu não desenha bandeira nenhuma, em vez do país que
calhou de vir antes.

### Categorias

Não existe gerenciador de categorias, e isso é de propósito. Uma é criada
digitando o nome no diálogo de adição e escolhendo para onde aquele download
vai; daí em diante o nome **significa** aquela pasta. Uma tela separada para
criar algo cuja definição inteira são dois campos já na tela seria um segundo
lugar para fazer a mesma coisa.

Digitar um nome que o app já conhece move o destino para a pasta daquela
categoria. Digitar o mesmo nome com outra pasta **move a categoria**, não cria
uma segunda: duas "Séries" apontando para lugares diferentes é um sistema que
ninguém consegue prever. "TV Shows", "tv shows" e "TV  Shows" são uma só, e o
que o trilho mostra é a grafia digitada primeiro.

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

A lista tem **a pasta no topo**, do jeito que um gerenciador de arquivos mostra
uma pasta acima do que tem dentro. Três controles nessa linha, cada um com o
próprio alvo: o **chevron** dobra a lista para fora do caminho, o **tique** é
todos os arquivos de uma vez, e o nome não é nenhum dos dois.

O tique vai nos **dois sentidos**, como o do Gmail: liga tudo quando algo está
desligado, desliga tudo quando está tudo ligado. Ele só ia num sentido antes,
porque um torrent que não baixa nada era recusado — e um controle que funciona
numa direção só lê como um controle que não funciona.

**Não baixar nada é uma resposta de verdade**: manter o que está no disco,
compartilhar, e não pegar mais. A coluna de estado passa a dizer
`Nenhum arquivo selecionado` em vez de o clique sumir.

Cada linha carrega **o ícone que o próprio sistema mostra** para aquele tipo de
arquivo. Não um desenhado aqui: um `.mkv` no Zerem leva a mesma figura que leva
no Explorer, porque essa figura é o que *você* instalou para abri-lo, e um
desenho nosso seria uma segunda opinião sobre algo que a máquina já respondeu.
É perguntado pela extensão e **o arquivo nunca é tocado**, então funciona antes
de qualquer byte existir no disco.

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

**Cada torrent com mais de um arquivo ganha uma pasta com o nome dele**, dentro
da pasta de destino — como em qualquer outro cliente. Um torrent de arquivo
único vai direto: pasta com um arquivo dentro é pasta que ninguém pediu.

Essa regra é do librqbit, e ele a aplica sozinho — **até alguém dizer onde
escrever**. Passar `output_folder` explícito leva ao ramo que pula a subpasta
inteira. E o Zerem precisa passar: a pasta de destino muda com o app rodando, e
o librqbit fixa a dele na construção, sem setter. Então a regra é reimplementada
em [`folder.rs`](crates/zerem-core/src/folder.rs), onde dá para testá-la.

O nome do torrent vem de um estranho, então um nome que não seja **exatamente um
componente de caminho** — `..\..\Windows`, `C:\`, qualquer barra — perde a
subpasta em vez de derrubar o torrent: os arquivos caem soltos num lugar
inofensivo em vez de num lugar escolhido por quem montou o torrent.

## A lista de estados

À esquerda, uma linha por estado com a contagem ao lado: **Todos**, **Baixando**,
**Semeando**, **Na fila**, **Pausados**, **Com erro**. A contagem é o que paga a
largura — ela responde "tem alguma coisa travada?" sem um clique, que é a
pergunta pela qual alguém abre um cliente de torrent depois do almoço.

Some com o botão na barra, e o estado disso sobrevive a fechar e reabrir. Ela
combina com o filtro de texto em vez de brigar: as duas perguntas são
diferentes, e uma tem de sobreviver à outra sendo digitada.

`Ctrl+A` seleciona **o que a lista está mostrando** — nunca uma linha que ela
está escondendo, o que faria o `Delete` agir sobre algo que ninguém vê. Duplo
clique numa linha abre a pasta, igual ao `Enter`.

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

## Quando termina

Um download que acaba é dito na barra de status pelo nome, e o botão do app
**pisca na barra de tarefas** se a janela não estiver na frente. Nada rouba o
foco: interromper o que a pessoa está fazendo para anunciar que um arquivo
chegou é o comportamento que faz todo mundo desligar notificação.

Se a janela já estiver na frente, não pisca — é um jeito de dizer "quando você
voltar", e quem está olhando já voltou. Vários terminando no mesmo segundo viram
uma linha só: quatro avisos em quatro segundos se empurram antes de qualquer um
ser lido.

É o anúncio que dá para fazer hoje. Um *toast* de verdade quer um AppUserModelID
e um atalho no menu Iniciar para ser atribuído, o que significa uma cópia
instalada — ele entra com o instalador.

### Mover para outra pasta ao terminar

Desligado até alguém nomear uma pasta, em **Downloads → Mover para cá ao
terminar**. Baixar num disco rápido e guardar num grande é a razão inteira de a
opção existir.

O librqbit não tem `move_storage`. A libtorrent tem — é assim que o qBittorrent
move um torrent pronto sem ler um byte: avisa a biblioteca que os arquivos estão
em outro lugar e ela continua semeando. Aqui o único jeito de mudar onde um
torrent vive é deixar de ser aquele torrent e virar um novo apontando para o
lugar novo. Então é uma sequência, e a ordem é o projeto inteiro:

1. Guardar o que a reconstrução precisa — os bytes do `.torrent`, quais arquivos
   eram desejados, onde estão e para onde vão.
2. Pausar, para nada estar escrevendo enquanto os arquivos se movem.
3. Mover, **fora da thread do motor**. É a parte lenta e a única que pode perder
   dados, então é a que se desfaz sozinha: mesmo volume é um `rename` por
   arquivo; entre volumes, cada arquivo é copiado e conferido no tamanho antes
   de qualquer coisa ser apagada.
4. **Só depois de o movimento passar**, largar o torrent sem tocar nos arquivos
   e adicioná-lo de novo na pasta nova.

Toda falha antes do passo quatro deixa um torrent que continua na lista,
continua sabendo onde estão seus arquivos, e simplesmente não se moveu. Nada é
largado antes de o dado já estar do outro lado.

**O preço, dito com todas as letras:** o torrent é reverificado. `overwrite` é o
que permite ao librqbit retomar ou semear um torrent cujas peças já estão
escritas, e ele confere para descobrir. Um torrent de cinquenta gigabytes lê
cinquenta gigabytes de volta depois de se mover, e aparece como **Verificando**
até terminar. Não há como contornar isso de fora da biblioteca. Paga-se uma vez,
depois de o download já ter acabado, sem ninguém esperando pelo conteúdo.

## Pasta vigiada

Um `.torrent` largado numa pasta escolhida é adicionado sem ninguém abrir a
janela — de um navegador, de um script, de um compartilhamento de rede. É a
automação mais antiga que um cliente de torrent tem, e é o que faz dele um
serviço em vez de um aplicativo.

O arquivo é **renomeado, nunca apagado**: `.torrent.added` quando entrou,
`.torrent.failed` quando não. O segundo nome importa mais do que parece — um
arquivo quebrado deixado onde estava é recolhido de novo na varredura seguinte,
falha de novo e reclama de novo, para sempre.

A varredura acontece a cada quatro tiques e não a cada um: uma listagem de
diretório por segundo numa pasta que quase sempre está vazia é uma chamada de
sistema gasta em nada, e quatro segundos estão muito dentro do tempo que alguém
leva para notar que um download não começou.

Só o final exato `.torrent` é recolhido, o que também é o que mantém um arquivo
pela metade de fora: um navegador baixa para `algo.torrent.crdownload` e só
renomeia quando os bytes chegaram todos.

## Assistir antes de terminar

**Play now** no menu de um arquivo de vídeo pede as peças daquele arquivo na
ordem e abre um servidor no loopback, numa porta que o sistema escolhe. O player
recebe uma URL comum, com `Range` e `206`, então qualquer coisa que toque vídeo
pela rede serve — e o motor prioriza as peças que o fluxo está pedindo.

O caminho é `/t/{id}/{índice}`: um **índice numérico**, não um caminho de
arquivo. Travessia de diretório é impossível por construção, não por filtro.

## Atualizações

Em **Sobre**, e conferidas sozinhas alguns segundos depois de abrir. O app lê um
feed publicado com cada release, baixa só o binário novo, **confere a assinatura
minisign** contra uma chave compilada dentro dele e troca o executável no lugar.
Um download que não passa na assinatura é descartado antes de tocar o disco.

HTTPS diria que os bytes vieram do GitHub sem alteração e não diria quem os pôs
lá — que é a única pergunta que importa quando a resposta decide o que roda como
você na próxima vez que abrir o app.

A checagem automática **cala a boca quando falha**. Um notebook aberto antes do
wifi associar não tem nada a dizer, e "erro ao enviar requisição" é resposta a
uma pergunta que ninguém fez. Só o botão reporta erro.

E o botão não aparece onde não funcionaria: numa cópia instalada para todos, ele
diria de onde vêm as atualizações em vez de baixar, verificar e falhar no sistema
de arquivos de quem clicou.

## Bandeja

**Fechar a janela esconde, não encerra.** Um cliente que para de semear porque a
janela incomodava está fazendo a coisa errada. Sair é pelo menu da bandeja.

Isso só é aceitável por causa do trabalho já feito: com a janela escondida o
tick da UI **para** — a Fase 0 mediu esse estado em 0,000 % de CPU — enquanto a
engine continua transferindo. Clique esquerdo no ícone alterna a janela.

A engine continua fazendo mais do que transferir, e isso é preciso ser exato: ela
segue montando um retrato a cada segundo e jogando fora, porque montá-lo também é
como ela **percebe que um torrent terminou**. Um download que acaba com a janela
na bandeja é movido para onde os concluídos ficam, e é anunciado quando a janela
volta. Pular esse passo era mais barato e estava errado.

E se a bandeja não subir — no Windows a área de notificação some por um segundo
sempre que o shell reinicia — **o botão fechar encerra em vez de esconder**.
Esconder sem ter de onde voltar deixa alguém com um processo que não alcança.

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
cd vendor/librqbit; cargo test --lib -- --test-threads=1   # o motor patcheado
```

O último é à parte porque o `vendor/` está fora do workspace — nossos lints não
têm o que fazer sobre código de terceiros. Ele carrega o único teste que prova a
criptografia numa rede: duas sessões, ambas recusando texto claro, baixando uma
da outra. As portas são fixas, então os testes não dividem máquina entre si.

O compilador é fixado em [`rust-toolchain.toml`](rust-toolchain.toml): sem isso,
cada versão nova do Rust deixa o CI vermelho em código que ninguém tocou.

## Componentes de terceiros

| Componente | Termos |
|---|---|
| **Slint** (toolkit de UI) | Licenciamento próprio — GPLv3, royalty-free desktop, ou comercial. Um binário distribuído tem de estar coberto por uma delas; ver [slint.dev](https://slint.dev). **A escolha para o Zerem é decisão da Fase 4, antes do primeiro release.** |
| **librqbit** (engine BitTorrent) | Apache-2.0. Fonte em [github.com/ikatson/rqbit](https://github.com/ikatson/rqbit). **Uma cópia patcheada vive em [`vendor/librqbit`](vendor/librqbit)** — ele não tem costura por onde passar criptografia de protocolo, e [`vendor/CHANGES.md`](vendor/CHANGES.md) lista cada alteração. |

## Limitações conhecidas

**A criptografia de protocolo é só sobre TCP.** Ela existe — está em
[`crates/zerem-mse`](crates/zerem-mse), com a troca de chaves, o RC4 e o aperto
de mão dos dois lados, e o download cifrado ponta a ponta é provado por um teste
no motor vendorizado. Mas sobre uTP o aperto de mão completa e o fluxo de
mensagens trava, e a causa não foi encontrada. Então o uTP é desligado enquanto a
criptografia estiver ligada, e o painel de Conexão diz isso na própria linha.

Por isso ela vem **desligada por padrão**. Oferecê-la não custa peers — cai para
texto claro com quem não aceita — mas custa o uTP, que é o que impede o cliente
de tomar a linha inteira. Ligue-a se o seu provedor molda BitTorrent ou se um
tracker exige.

**Trackers privados filtram por `peer_id`**, e um cliente próprio não passa no
whitelist deles. A criptografia removeu um dos dois motivos pelos quais eles
recusariam o Zerem; o `peer_id` continua sendo o outro.

Duas limitações ficam dentro do motor: a escolha de peças é por ordem de arquivo
e **não rarest-first**, e não há *fast extension*. Nenhuma das duas é uma decisão
deste projeto; são o que o librqbit cobre hoje.

## Licença

MIT © Ohgawa.

**Ainda não resolvido, e bloqueia o primeiro release:** o Slint é distribuído sob
GPLv3, sob uma licença royalty-free para desktop, ou comercial. Um binário
publicado precisa estar coberto por uma delas, e a escolha muda o que o MIT acima
significa na prática. Está registrada como risco conhecido no
[roadmap](ROADMAP.md).
