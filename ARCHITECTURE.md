# Zerem — Arquitetura

O detalhe técnico por trás do [ROADMAP.md](ROADMAP.md). Este documento é o
contrato: quando o código e ele discordarem, um dos dois está errado e a
discordância se resolve antes do merge.

---

## 1. Camadas e a regra de dependência

```
┌──────────────────────────────────────────────────────────┐
│  zerem (bin)          Slint + bridge/                    │
│                       conhece engine e core              │
├──────────────────────────────────────────────────────────┤
│  zerem-engine         librqbit + tokio                   │
│                       conhece core. NÃO conhece Slint.   │
├──────────────────────────────────────────────────────────┤
│  zerem-core           tipos, bencode, ETA, rate math,    │
│                       sort/filter, formatação            │
│                       sem async, sem I/O, sem UI         │
└──────────────────────────────────────────────────────────┘
```

**A regra:** as setas apontam só para baixo. `zerem-core` não tem `tokio` nem
`slint` no `Cargo.toml` — e é isso que o mantém testável sem runtime e sem
janela. `zerem-engine` não sabe que existe UI; ele publica estado e consome
comandos, e serviria igualmente a um daemon headless na Fase 5.

Essa fronteira é o que torna o modo headless (backlog) quase de graça: ele é o
mesmo `zerem-engine` com outro consumidor de `Snapshot`.

---

## 2. O contrato central: um snapshot por segundo

Todo o estado que a UI desenha atravessa **uma única estrutura imutável**,
publicada a cada tick. Não há a UI perguntando nada ao engine no meio do caminho.

```rust
/// Published once per tick. Immutable; the UI never mutates it.
pub struct Snapshot {
    pub seq: u64,
    pub session: SessionStats,
    pub torrents: Vec<TorrentRow>,
}

/// One row of the main table. Flat and Copy-friendly on purpose: the diff in
/// the UI model compares these field by field, once per tick per row.
pub struct TorrentRow {
    pub id: TorrentId,
    pub name: Arc<str>,       // shared, never re-allocated per tick
    pub size: u64,
    pub done: u64,
    pub state: TorrentState,
    pub down_bps: u64,
    pub up_bps: u64,
    pub seeds: (u32, u32),    // connected, total
    pub peers: (u32, u32),
    pub eta_secs: Option<u32>,
    pub ratio: f32,
}
```

**Direções.** Estado desce (`Snapshot` → UI). Comandos sobem (UI → `Command`).
Nunca o contrário, nunca um atalho. É literalmente a regra "state flows downward,
events flow upward" do CLAUDE.md, aplicada com um tipo só.

**`Arc<str>` no nome** importa mais do que parece: o nome não muda entre ticks, e
clonar `String` de 2000 linhas por segundo é 2000 alocações/s para nada.

**Detalhes são fora do snapshot.** Peers, arquivos e trackers **não** entram no
`Snapshot` — são consultados sob demanda, só para o torrent selecionado e só com
o painel aberto. Puxar a lista de peers de 500 torrents por segundo é o erro que
faz cliente torrent comer CPU sem baixar nada.

**O tick pausa.** Janela minimizada ou escondida na bandeja ⇒ o ticker para. Não
diminui a frequência: para. É o que sustenta o alvo de ≤ 0,1 % de CPU na bandeja.

---

## 3. O modelo da tabela — a decisão de performance da UI

O erro que mata o frame rate é recriar um `VecModel` a cada tick: realoca tudo,
descarta a posição do scroll e perde a seleção. O modelo é **custom, em Rust, com
notificação por linha.**

```rust
pub struct TorrentModel {
    rows: RefCell<Vec<TorrentRow>>,  // current view, already sorted+filtered
    notify: ModelNotify,
}

impl TorrentModel {
    /// Apply a new snapshot with the minimum number of notifications.
    pub fn apply(&self, next: Vec<TorrentRow>) {
        let mut rows = self.rows.borrow_mut();
        if rows.len() != next.len() {
            // Row set changed (added/removed): reset, then re-select by id.
            *rows = next;
            drop(rows);
            self.notify.reset();
            return;
        }
        for (i, (old, new)) in rows.iter_mut().zip(next).enumerate() {
            if *old != new {
                *old = new;
                self.notify.row_changed(i);   // repaints one row, not 2000
            }
        }
    }
}
```

Consequências que valem como regra:

- **Ordenação e filtro acontecem no Rust**, antes do `apply`. O `.slint` recebe a
  lista já na ordem final. Nenhuma expressão de ordenação dentro do `.slint` —
  ela reavalia a cada frame.
- **A seleção é por `TorrentId`, nunca por índice.** Índice muda quando a
  ordenação muda ou um torrent some; ID não.
- **`TorrentRow` deriva `PartialEq`** e a comparação é o coração do laço. Se um
  campo é flutuante e ruidoso (ratio), arredonde na origem — senão toda linha
  "muda" todo tick e o diff não economiza nada.

### O que a Fase 0 mediu

Números de [docs/spike-report.md](docs/spike-report.md), a 2000 linhas. As duas
constantes que governam tudo:

| | Custo |
|---|---|
| Comparar uma linha | ~0,03 µs |
| Formatar uma linha | ~1,1 µs (onze `SharedString`) |
| Tick completo, caso realista | **92 µs** |

**Comparar é de graça; formatar é o custo.** Daí as regras, todas medidas e
nenhuma teórica:

1. **Chave de ordenação dobrada na origem.** Dobrar maiúsculas dentro do
   comparador custou 3200 µs por ordenação — uma ordenação faz ~22 000
   comparações. Guardar `name_key` minúsculo na criação: 75 µs.
2. **Coluna cuja chave não pode mudar não é reordenada.** Nome e tamanho são fixos
   na vida do torrent; enquanto o conjunto de linhas não muda, a ordem não se
   move. É o padrão do app, então o caso comum não faz trabalho nenhum.
3. **Escrever propriedade de UI só quando o valor difere** — a forma escalar da
   mesma regra do diff.
4. **Formatar só o que está visível.** O `row_data` só é chamado pelas ~25 linhas
   na viewport. Com a formatação movida para lá, o tick passa a custar
   `O(mudou)` de comparação mais `O(visível)` de formatação, e o pior caso cai
   cerca de 30×.
5. **`Arc<str>` para todo texto que não muda entre ticks.** Clonar `String` de
   2000 linhas por segundo é 2000 alocações para um valor idêntico.

---

## 4. Comandos e a atualização otimista

```rust
pub enum Command {
    Add(AddRequest),
    Start(TorrentId),
    Pause(TorrentId),
    Remove { id: TorrentId, delete_data: bool },
    SetFilePriority { id: TorrentId, file: u32, prio: Priority },
    SetGlobalLimit { down: Option<u64>, up: Option<u64> },
    Reannounce(TorrentId),
}
```

Vão da UI para o engine por um `mpsc` sem bloquear. E o comando **aplica na UI
antes de o engine responder**:

```rust
// Optimistic: the row flips state now; the next snapshot confirms it.
model.set_state(id, TorrentState::Paused);
model.mark_pending(id, snapshot.seq);
commands.send(Command::Pause(id));
```

`mark_pending` guarda o `seq` do snapshot vigente. O primeiro snapshot com
`seq` maior é a verdade e sobrescreve o palpite — se o engine recusou o comando,
a linha volta sozinha. É esse mecanismo que entrega o "< 100 ms visível" da
tabela de alvos sem mentir sobre o estado real.

---

## 5. Threads — quem pode fazer o quê

| Thread | Faz | Nunca faz |
|---|---|---|
| **UI (event loop do Slint)** | desenhar, callbacks, aplicar snapshot | I/O de disco, rede, diálogo bloqueante, `metadata()` |
| **Tokio (multi-thread)** | librqbit, trackers, disco, HTTP | tocar em `ui.set_*` diretamente |
| **Ticker (task tokio)** | montar `Snapshot`, publicar via `invoke_from_event_loop` | lógica de negócio |
| **`spawn_blocking`** | diálogos nativos, varredura de diretório, lixeira, hash | qualquer coisa async |

A regra do `pick_async` do Vayou vale generalizada aqui: **nada que possa demorar
roda na thread da UI.** Num player, um diálogo bloqueante congela a imagem; num
cliente torrent, um `File::metadata()` sobre um torrent de 4000 arquivos engasga
o frame do mesmo jeito, e é muito mais fácil de escrever por acidente.

---

## 6. Persistência

Três arquivos, três regimes:

| Arquivo | Quando escreve | Regime |
|---|---|---|
| `session.json` — a lista de torrents | ao adicionar/remover e a cada 30 s | **atômico** |
| `resume/<id>.dat` — resume data do engine | pelo engine | **atômico** |
| `settings.json` | debounce de 400 ms + flush na saída | **atômico** |

**Escrita atômica não é opcional em nenhum dos três:**

```rust
/// Write via temp + rename. A crash mid-write leaves the previous file intact
/// instead of a truncated one — losing the torrent list once costs the user's
/// trust permanently.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    { let mut f = File::create(&tmp)?; f.write_all(bytes)?; f.sync_all()?; }
    fs::rename(tmp, path)
}
```

O debounce das preferências é o `persist.rs` do Vayou tal e qual (400 ms de
silêncio, `flush` depois de `run_event_loop_until_quit`) — os sliders de limite de
banda têm exatamente o mesmo problema que os de legenda.

---

## 7. Estado da UI — quatro globals, não um

```
ui/state.slint
├── global SessionState   totais, ↓/↑ agregado, sparkline, contagem por estado
├── global TorrentList    linhas, coluna e direção da ordenação, filtro, seleção
├── global DetailState    arquivos/peers/trackers do selecionado, aba ativa
└── global Prefs          tudo que aparece nas preferências
```

O `MainWindow` re-expõe os campos com `<=>` para o bridge Rust manter a API
`ui.set_*`/`get_*`, exatamente como o Vayou faz — mas repartido por domínio desde
o início. Um global único cresce até ~90 propriedades e aí ninguém mais sabe quem
escreve o quê.

**Zero lógica no `.slint`.** Formatação de bytes, de velocidade e de ETA vem
pronta do Rust em `SharedString`. O motivo não é purismo: a função de formatar
avaliada dentro de um `for` no `.slint` roda por linha por frame.

---

## 8. Erros

`thiserror` em cada camada, mais o trait `LogErr` do Vayou para os casos em que a
falha é registrada e a vida segue. **Nenhum `unwrap`/`expect` fora da
inicialização** — e o que aparece na UI é sempre uma frase que diz o que fazer,
não um código:

```rust
#[derive(thiserror::Error, Debug)]
pub enum AddError {
    #[error("not enough disk space: {needed} more required")]
    DiskFull { needed: ByteSize },   // → oferece "Escolher outra pasta"
    #[error("invalid magnet link")]
    BadMagnet,
    #[error("already in the list")]
    Duplicate(TorrentId),            // → seleciona e destaca a linha existente
}
```

Cada variante corresponde a uma recuperação desenhada na UI. Uma variante sem
recuperação é um bug de design, não de código.

---

## 9. Layout do repositório

```
Zerem/
├── crates/
│   ├── zerem-core/        tipos · bencode · ETA · rate · sort/filter · fmt
│   ├── zerem-engine/      librqbit (uTP on) · session · disk · ticker
│   └── zerem-shell/       updater · single-instance · tray · componentes Slint
├── src/
│   ├── bridge/            torrents · detail · files · peers · trackers
│   │                      prefs · window · persist · keys        (um wire() cada)
│   ├── services/          lógica pura da camada de app
│   ├── model.rs           TorrentModel (§3)
│   ├── win/               chrome nativo, um módulo por plataforma
│   └── main.rs            ≤ 250 linhas
├── ui/
│   ├── state.slint        os quatro globals
│   ├── theme.slint        tokens M3 semânticos
│   ├── components/        vindos do Clipo
│   └── windows/           main · add-torrent · prefs
├── lang/<code>/LC_MESSAGES/zerem.po
├── installer/             NSIS por usuário · build.ps1 · latest.json
└── docs/                  spike-report.md · benchmarks.md
```

---

## 10. Configuração de build

```toml
[profile.release]
opt-level = "s"      # tamanho: o binário é parte do produto
lto = true
strip = true
panic = "abort"
codegen-units = 1

[profile.dev.package."*"]
opt-level = 2        # dependências otimizadas: engine em debug não satura o link
```

`[profile.dev]` com `debug = "line-tables-only"` para o link incremental ficar
curto, igual ao Vayou. Deu **9,08 MB** de binário no spike, com o Slint inteiro
dentro.

**Renderizador: `winit-software`.** Ao contrário de Clipo e Vayou, que precisam do
femtovg (um compõe imagem, o outro põe vídeo sob a UI), o Zerem desenha texto e
retângulos e ganha com a repintura parcial. Medido contra o femtovg: **17 MB de
memória privada contra 120 MB, e 3,9 % de um núcleo contra 20,9 %** sob carga de
repintura. O femtovg fica disponível via `SLINT_BACKEND` para quem tiver um caso
onde ganhe.

Os lints seguem a lista curta do Clipo, com a família de `cast` liberada e
justificada: todo valor que cruza para o Slint muda de largura por necessidade —
contagens de bytes são `u64` e a barra de progresso quer `f32`, ids são `u32` e o
`int` do Slint é `i32`. Nada além disso é dispensado, e um `allow` novo vem com o
motivo escrito ao lado.

---

## 11. Regras invioláveis

O checklist que um diff precisa passar. Qualquer "não" aqui é bloqueio, não
ressalva.

- [ ] `main.rs` tem ≤ 250 linhas
- [ ] Nenhum I/O na thread da UI
- [ ] Nenhuma lógica dentro de um `.slint`
- [ ] Ordenação e filtro acontecem no Rust
- [ ] O modelo notifica por linha, nunca com `reset` num tick comum
- [ ] Seleção referenciada por `TorrentId`, nunca por índice
- [ ] Nenhum float no valor de domínio — `ratio` é `u32` em centésimos
- [ ] Chave de ordenação dobrada na origem, nunca dentro do comparador
- [ ] Propriedade de UI escrita só quando o valor difere
- [ ] Toda escrita em disco é atômica
- [ ] Todo comando aplica otimista antes da confirmação
- [ ] O ticker para quando a janela não está visível
- [ ] Todo `allow` de clippy novo traz o porquê no comentário
- [ ] Todo erro visível ao usuário tem uma ação de recuperação ao lado
