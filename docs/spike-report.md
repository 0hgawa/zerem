# Zerem — Fase 0, relatório dos spikes

**Data:** 2026-09-02 · **Máquina:** Windows 11 LTSC, 24 threads
**Stack:** Rust 1.98.0 (MSVC) · Slint 1.17.1 · perfil `release`
**Código:** [`spikes/table/`](../spikes/table/) — 23 testes, clippy pedantic + nursery limpo

| Spike | Veredito |
|---|---|
| **S0.1** — tabela virtualizada | ✅ **Aprovado**, com uma verificação manual pendente |
| **S0.3** — tick por snapshot | ✅ **Aprovado** |
| **S0.2** — engine (librqbit + uTP) | ✅ **Aprovado** — satura o link, uTP confirmado; o alvo de CPU é que estava errado |

Duas decisões do plano mudam por causa do que foi medido: **o renderizador padrão
passa a ser o de software**, e **a meta de memória sobe de ≤ 60 MB para um piso
medido**. Detalhe abaixo.

---

## 1. Custo do tick

Cada linha é a média de ~14 ticks a 1 Hz. `changed` é quanto o diff sujou;
`moved` (o que a sessão de fato mexeu) bateu com ele **exatamente em todos os
casos** — o diff não suja nada a mais.

| Caso | Linhas | changed | Ordenação | Diff médio | Diff máx | reset |
|---|---|---|---|---|---|---|
| Nome, realista | 2 000 | 26 | *pulada* | **92 µs** | 100 µs | 0 |
| Velocidade, realista | 2 000 | 26 | 75 µs | 274 µs | 294 µs | 0 |
| Nome, churn | 2 000 | 1 544 | *pulada* | 1 831 µs | 2 604 µs | 0 |
| Velocidade, churn | 2 000 | 2 000 | 81 µs | 2 360 µs | 2 644 µs | 0 |
| Nome, lista pequena | 200 | 1 | *pulada* | 12 µs | 16 µs | 0 |
| Velocidade | 10 000 | 195 | 449 µs | 562 µs | 611 µs | 0 |
| Velocidade, churn | 10 000 | 10 000 | 561 µs | 10 790 µs | 14 066 µs | 0 |

**`reset = 0` em toda a matriz.** Nenhum tick reconstruiu o modelo, o que é o que
preserva a posição do scroll e a seleção. O alvo do plano (diff < 1 ms a 2 000
linhas) é batido com folga de 10×: **92 µs**.

Decompondo o custo, as duas constantes que importam:

- **comparar uma linha: ~0,03 µs** — inteiros e um `Arc<str>` ponteiro-igual
- **formatar uma linha: ~1,1 µs** — onze `SharedString` alocadas

O diff é praticamente de graça; **o custo é a formatação**, e ele escala com o
número de linhas que mudaram, exatamente como projetado.

### O caso que estoura o orçamento

`churn` a 10 000 linhas custa **10,8 ms** e perderia quadro. Mas isso é *toda*
linha de uma lista de dez mil mudando no mesmo segundo — um cenário que não
existe num cliente real. O caso realista a 10 000 linhas custa 562 µs.

Mesmo assim há a correção certa, e ela é barata: **formatar sob demanda.** O
`row_data` só é chamado pelas ~25 linhas visíveis; se a formatação sair do `apply`
e for para lá, o tick passa a custar `O(mudou)` de comparação mais `O(visível)`
de formatação. Pelos números acima isso derruba o pior caso em cerca de 30×.
→ **Fase 1.**

---

## 2. A ordenação era o gargalo escondido

A primeira medição deu **3 200 µs por tick só na ordenação** — cinquenta vezes o
diff inteiro. Duas correções, ambas incorporadas:

| | Antes | Depois |
|---|---|---|
| Ordenar por nome, 2 000 linhas | 3 200 µs | **pulada (0)** |
| Ordenar por coluna volátil, 2 000 | 3 200 µs | **75 µs** |
| Ordenar por coluna volátil, 10 000 | — | 449 µs |

1. **Chave de nome dobrada uma vez.** Comparar sem alocar com
   `chars().flat_map(char::to_lowercase)` parecia esperto e era o problema: uma
   ordenação faz ~22 000 comparações e cada uma reprocessava as duas strings
   pelas tabelas Unicode. Guardar `name_key` minúsculo na criação transforma
   isso em comparação de bytes — **43× mais rápido**.
2. **Não reordenar o que não pode ter mudado.** Nome e tamanho são fixos na vida
   do torrent; enquanto o conjunto de linhas não muda, a ordem por essas colunas
   não se move. E é o padrão do app, então o caso comum não faz trabalho nenhum.

Ordenar por velocidade também **triplica o diff** (92 → 274 µs), o que é correto
e esperado: quando linhas trocam de posição, a posição *i* passa a conter outro
torrent e é legitimamente suja.

---

## 3. Renderizador — a descoberta que muda o plano

Mesmo binário, mesma carga, só trocando `SLINT_BACKEND`:

| | femtovg (GPU) | **software** | |
|---|---|---|---|
| Working set | 90,6 MB | **33,4 MB** | −63 % |
| Memória privada | 120,3 MB | **17,3 MB** | −86 % |
| CPU a 1 Hz (1 núcleo) | 1,95 % | **0,98 %** | −50 % |
| CPU a 30 Hz (1 núcleo) | 20,88 % | **3,90 %** | −81 % |
| Diff (lado CPU) | 92 µs | 108 µs | igual |

O renderizador por software ganha em **todos** os eixos medidos, e por margem
larga. A explicação é a renderização parcial: ele repinta só a região suja,
enquanto o caminho GPU redesenha o quadro inteiro. Numa tabela onde 26 de 2 000
linhas mudam por segundo, quase tudo está limpo.

**Decisão: o renderizador por software passa a ser o padrão do Zerem.** O
femtovg fica como opção via `SLINT_BACKEND` para quem tiver um caso onde ganhe.

**A verificação que falta, e é manual:** rolagem. A repintura parcial não ajuda
quando a viewport inteira se move, e é o único cenário que exige mão no mouse.
Rodar o spike nos dois renderizadores e rolar a lista de ponta a ponta é o que
fecha o S0.1 — instruções em [`spikes/table/README.md`](../spikes/table/README.md).

Consequência para o código: o contador de quadros usa o *rendering notifier*, que
só os renderizadores GPU oferecem. Ele já degrada com um aviso em vez de derrubar
o app, mas medir quadros no modo software exige outro instrumento.

---

## 4. Memória — a meta do plano estava errada

| Linhas | Working set | Privada |
|---|---|---|
| 200 | 89,5 MB | 118,4 MB |
| 2 000 | 90,6 MB | 120,3 MB |
| 10 000 | 97,7 MB | 128,6 MB |

Com femtovg a memória é **quase plana** de 200 a 10 000 linhas: ~1,5 KB por
linha, contra um piso fixo de ~89 MB que é o contexto GL, os atlas de textura e o
AccessKit. Ou seja, **o custo não era a lista, era o renderizador** — e a meta de
≤ 60 MB do roadmap era inatingível por construção com GPU.

No renderizador por software o piso cai para **33,4 MB de working set / 17,3 MB
privados**, e aí a meta original não só é atingível como sobra espaço.

**Metas revisadas** (ver ROADMAP.md):

| Métrica | Antes | Agora | Medido |
|---|---|---|---|
| RAM ociosa, 10 torrents | ≤ 60 MB | ≤ 60 MB WS | 33 MB (software) |
| RAM, 1 000 torrents | ≤ 200 MB | ≤ 80 MB WS | ~35 MB extrapolado |
| Binário | ≤ 12 MB | ≤ 12 MB | **9,08 MB** ✅ |

---

## 5. CPU em repouso

| Situação | CPU (24 núcleos) |
|---|---|
| Janela aberta, **sem tick** (`ZEREM_TICK_MS=60000`) | **0,000 %** |
| Janela aberta, tick de 1 Hz, 2 000 linhas | 0,05 – 0,25 % |
| Janela aberta, tick de 1 Hz, 10 000 linhas | 0,35 % |

Com nada mudando o app não gasta absolutamente nada. Isso confirma a regra da
arquitetura de **parar o ticker quando a janela não está visível** — não é
"reduzir a frequência", é zerar o consumo, e o mecanismo já está provado.

---

## 6. Pendência aberta: ~12 quadros por tick

O contador registra **~12 repinturas por tick, constante** — 11,8 a cada 250 ms,
11,4 a cada 1 s, 10,7 a cada 4 s. Não escala com o tempo, escala com a mudança:
é um *burst* fixo por alteração, onde um quadro bastaria.

Duas hipóteses foram testadas e **descartadas**:

- **Escritas de propriedade.** Passar as nove propriedades de status a só
  escrever quando o valor muda não alterou nada (11,8 → 11,4). A otimização ficou
  no código porque está certa de qualquer forma, mas não era a causa.
- **A animação do chip.** Remover `animate background` não alterou a contagem.

Restam como suspeitos o `ListView` reassentando a viewport e o próprio notifier
sendo chamado mais de uma vez por quadro apresentado. **Não é bloqueante** — o
CPU já está em 0,05 %, e no renderizador por software a repintura é parcial de
qualquer forma. Mas há um ganho de ~10× em repinturas esperando ser recolhido.
→ **Fase 1.**

---

## 7. S0.2 — engine

**Executado.** Alvo: `debian-13.6.0-amd64-DVD-1.iso` (3,72 GB), o sujeito
convencional — muitos seeds e legal de buscar. Cerca de 22 GB de tráfego ao
todo, apagados depois.

### As duas perguntas binárias passam

**Satura o link.** O ISO inteiro em **96 segundos**, pico de **66,9 MB/s**
(~535 Mbps). A rampa é rápida: 40 MB/s em 4 s, regime estável a partir de t=5.

**O uTP funciona.** Até **15 peers uTP** simultâneos ao lado de ~100 TCP, medidos
por `live_utp` e não inferidos de configuração.

### As medições

Debian DVD-1, corridas de 60 s, CPU amostrado em regime estável. `cpu` é
percentagem de **um** núcleo (a máquina tem 24).

| Cenário | CPU | Taxa | WS | cpu por MB/s |
|---|---|---|---|---|
| Baixando, TCP+uTP | 72 – 79 % | 45 – 47 MB/s | 43 MB | 1,53 |
| Baixando, só TCP | 66,7 % | 45 – 46 MB/s | 39 MB | 1,49 |
| Baixando, `--peer-limit 40` | 64,5 % | 45,9 MB/s | 31 MB | 1,41 |
| **Semeando (ocioso)** | **0,77 %** | 0,03 MB/s ↑ | **25,5 MB** | — |

### O alvo de CPU estava errado, e o engine não

O roadmap pedia **≤ 15 % de um núcleo saturando 1 Gbps**. O medido é
~**1,45 % de um núcleo por MB/s**, linear — o que extrapola para ~1,8 núcleos a
1 Gbps, cerca de doze vezes o alvo.

Duas hipóteses testadas para explicar o custo, **ambas descartadas**:

1. **Backend de SHA-1.** O wrapper do próprio librqbit diz *"Sha1 computation is
   the majority of CPU usage of librqbit"*, e a feature `rust-tls` troca o
   backend padrão por `aws-lc-rs` (caminho SHA-NI). Trocado e medido: 1,49 → 1,53
   normalizado. **Sem diferença** — o `crypto-hash` padrão já usa o CNG do
   Windows, que também é acelerado por hardware.
2. **Gerência de conexões.** As primeiras corridas mantinham ~128 conexões
   abertas. Limitado a 40: 1,49 → 1,41. **Dentro do ruído.**

O custo é linear em bytes e não se move com configuração. Então o alvo de 15 %
foi inventado sem dado e é o que está errado — não o engine. **Metas revisadas:**

| | Antes | Agora |
|---|---|---|
| Baixando | ≤ 15 % de um núcleo a 1 Gbps | ≤ 1,5 % de um núcleo por MB/s |
| **Semeando** | não existia | **≤ 1 % de um núcleo** — medido 0,77 % |

A segunda é a que importa de verdade: um cliente de torrent passa quase toda a
vida semeando, e nesse estado o Zerem custa **0,77 % de um núcleo e 25 MB**.
Baixar a 45 MB/s custar 70 % de um núcleo por alguns minutos é o preço de estar
baixando a 45 MB/s.

> **Ainda em aberto:** se 1,45 %/MB/s é bom ou ruim em absoluto só se sabe
> comparando com outro cliente no mesmo link. Vale medir o qBittorrent baixando
> o mesmo ISO antes de tratar isto como fechado.

### O que mais apareceu rodando

- **`overwrite: true` não é opcional.** Do doc do próprio librqbit: *"Even when
  all the torrent pieces have been written, `overwrite` needs to be enabled in
  order to resume/seed the torrent."* Sem ele, restaurar a sessão no arranque
  falha com "file exists" em todo torrent concluído. Descoberto do jeito ruim:
  a segunda corrida abortou nos diretórios da primeira.
- **O arquivo é pré-alocado inteiro.** Uma corrida de 60 s já ocupava os 3,72 GB
  do ISO. É o comportamento certo — evita fragmentação — mas significa que
  adicionar um torrent consome o tamanho total na hora, e é por isso que a
  checagem de espaço em disco tem de ser **antes** de aceitar, não durante.
- Um aviso no encerramento: `error accepting uTP: dispatcher dead`. Ordem de
  desligamento do librqbit, sem efeito no resultado, mas anotado.

### E o que se sabia antes de tocar a rede

O spike está em [`spikes/engine/`](../spikes/engine/): compila, tem 6 testes, e
**se recusa a iniciar sem um torrent explícito** — não existe padrão nem corrida
acidental. Tudo abaixo foi lido da **fonte do crate**, não da documentação: a
doc gerada não nomeia metade disto e duas das armadilhas teriam passado.

`librqbit 9.0.1` resolve com `librqbit-utp 0.7.0` na árvore (361 crates ao todo).
Tudo abaixo foi lido da **fonte do crate**, não da documentação — a doc gerada
não nomeia metade disto, e duas das armadilhas teriam passado.

### Os padrões estão errados para um cliente

```rust
impl Default for ListenerOptions {
    fn default() -> Self {
        Self {
            // TODO: once uTP is stable upgrade default to both
            mode: ListenerMode::TcpOnly,
            listen_addr: (Ipv6Addr::UNSPECIFIED, 0).into(),
            enable_upnp_port_forwarding: false,
            ...
```

Três coisas, e nenhuma é detalhe:

1. **uTP vem desligado**, e o comentário do próprio autor diz que vira padrão
   "once uTP is stable" — ou seja, ele considera que ainda não é. É o maior
   risco que resta no projeto e o motivo de S0.2 existir.
2. **A porta padrão é `0`**, efêmera. Serve para conexões de saída e custa
   silenciosamente todas as de entrada — a diferença entre sugar e semear.
3. **UPnP vem desligado.**

O `listen_addr` em IPv6 unspecified é dual-stack, e `ipv4_only: false` mantém —
esse padrão está certo.

### Três armadilhas de tipo

| | O que parece | O que é |
|---|---|---|
| `Speed { mbps: f64 }` | megabits por segundo | **mebibytes** por segundo — o próprio `as_bytes()` multiplica por 1024². Ler como megabits erra toda velocidade da UI por **8×** |
| `TorrentStatsState::Error` | carrega o motivo | o motivo está num campo *irmão*, `TorrentStats::error`. Casar só no estado dá uma linha vermelha sem nada em que agir |
| `LiveStats::time_remaining` | um `Duration` | `DurationWithHumanReadable(Duration)` — campo privado, sem acessor, sem `Deref`. **Só dá para imprimir.** O ETA passou a ser calculado por nós, que é onde ele devia estar de qualquer forma: os segundos precisam de histerese antes de chegar a uma coluna |

### O uTP se verifica, não se supõe

`AggregatePeerStats` tem `live_tcp`, `live_utp` e `live_socks` — contagem de
peers vivos por tipo de conexão. Então o critério do S0.2 é uma leitura direta
(`live_utp > 0`), não uma inferência a partir da configuração. O spike acumula
isso ao longo da corrida e o resumo final diz, em letras claras, se nenhum peer
chegou por uTP.

### Uma lacuna no domínio que isto expôs

`TorrentStatsState::Error` não tinha para onde ir: `zerem_core::State` não tinha
`Error`, e `TorrentRow` não tinha campo para a mensagem. Escrever o mapeamento
foi o que revelou — o compilador exigiu um `match` total.

Corrigido no core, não no adaptador: `State::Error`, `TorrentRow::error`,
`fail(reason)` e `status_text()`. A coluna de estado passa a mostrar **a
mensagem** em vez da palavra "Error" — a cor já diz que algo está errado, então
o texto fica livre para dizer o quê. `pause()` e `resume()` limpam o erro, senão
uma falha antiga ficaria pendurada sob um torrent que voltou a rodar.

### O único critério que sobra é humano

Saturação e uTP estão medidos. O terceiro — **a latência da navegação continuar
intacta enquanto semeia** — é o motivo de o uTP existir (LEDBAT cede a fila ao
tráfego interativo) e não há como medir sem alguém navegando. Nas corridas aqui
o upload médio foi de 0,03 MB/s, ou seja o uplink nunca chegou perto de encher,
então o teste ainda não aconteceu de fato.

Como reproduzir tudo: [`spikes/engine/README.md`](../spikes/engine/README.md).

---

## 8. O que entra na Fase 1

Não é código descartável: estes quatro arquivos migram do spike para o app.

| Arquivo | Vira |
|---|---|
| [`src/model.rs`](../spikes/table/src/model.rs) | `src/model.rs` — o `TorrentModel` inteiro |
| [`src/sort.rs`](../spikes/table/src/sort.rs) | `zerem-core` — ordenação, `is_volatile`, chave dobrada |
| [`src/fmt.rs`](../spikes/table/src/fmt.rs) | `zerem-core` — toda a formatação |
| [`ui/theme.slint`](../spikes/table/ui/theme.slint) | `ui/theme.slint` — os tokens |

**Regras que o spike promoveu de opinião a medição** (já em ARCHITECTURE.md):

1. Chave de ordenação dobrada na origem, nunca dentro do comparador.
2. Coluna cuja chave não pode mudar não é reordenada.
3. Escrever propriedade de UI só quando o valor difere.
4. Formatar só o que mudou — e, a partir da Fase 1, só o que está visível.
5. `Arc<str>` para todo texto que não muda entre ticks.
6. Nada de float no valor de domínio: `ratio` é `u32` em centésimos.

**Itens abertos, todos verificações humanas ou afinação:**

- [ ] Rolar a lista à mão nos dois renderizadores e fechar o S0.1
- [ ] Navegar enquanto semeia com o uplink cheio, e ver se o LEDBAT cede a fila
- [ ] Comparar 1,45 %/MB/s contra o qBittorrent no mesmo link, para saber se é
      bom ou ruim em absoluto
- [ ] Formatação preguiçosa no `row_data`
- [ ] Investigar as ~12 repinturas por tick

**O que a Fase 2 tem de carregar junto com o mapeamento**, porque nada disto vem
de graça nos padrões do librqbit:

```rust
ListenerOptions {
    mode: ListenerMode::TcpAndUtp,          // padrão é TcpOnly
    listen_addr: (Ipv6Addr::UNSPECIFIED, port).into(),  // padrão é porta 0
    enable_upnp_port_forwarding: true,      // padrão é false
    ..Default::default()
}
AddTorrentOptions {
    overwrite: true,                        // padrão é false; sem ele não há resume
    ..Default::default()
}
```
