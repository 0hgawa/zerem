# Spike S0.2 — engine (librqbit)

Respondia as perguntas que sobraram da Fase 0. **As três passaram** — resultados
completos em [`../../docs/spike-report.md`](../../docs/spike-report.md):

| | |
|---|---|
| Satura o link? | ✅ 3,72 GB em 96 s, pico de 66,9 MB/s |
| O uTP conecta? | ✅ até 15 peers uTP, lidos de `live_utp` |
| Todo estado mapeia? | ✅ [`src/map.rs`](src/map.rs) é total |
| Semear é barato? | ✅ 0,77 % de um núcleo, 25 MB |

Fica aqui porque é reproduzível, e porque um critério continua aberto: **navegar
enquanto semeia com o uplink cheio**, que é o motivo de o uTP existir e não se
mede sem alguém navegando.

> ## Este spike toca a rede
>
> Ele entra num swarm BitTorrent real a partir do endereço desta máquina. Por
> isso **recusa iniciar sem um torrent explícito** — não há padrão e não há como
> rodar por acidente.

## Rodar

```powershell
cargo run --release -- <magnet-ou-url-ou-.torrent> [--out DIR] [--tcp-only] [--port N] [--seconds N]
```

O sujeito convencional é a imagem de uma distribuição Linux grande: muitos
seeds, legal de baixar, e grande o bastante para chegar a uma taxa estável.

| Flag | Padrão | Para quê |
|---|---|---|
| `--out DIR` | `%TEMP%\zerem-spike` | onde gravar |
| `--tcp-only` | desligado | comparar contra o caminho sem uTP |
| `--port N` | 6881 | porta de escuta fixa |
| `--seconds N` | sem limite | encerra sozinho, para medir sem babá |
| `--peer-limit N` | o do librqbit (~128) | testar se o custo é gerência de conexões (não é) |

Com `--seconds` o prazo manda: apontar para um diretório que já tem os dados
mede **semeadura** em vez de download, porque aí "concluído" é a condição
inicial e não um evento.

> O disco é **pré-alocado inteiro** ao adicionar. Uma corrida de 60 s num ISO de
> 3,7 GB já ocupa os 3,7 GB — conte o espaço antes, e apague os diretórios de
> saída depois.

`Ctrl-C` encerra e imprime o resumo. `ZEREM_LOG=debug` liga o log do librqbit.

## O que ler na saída

```
t=42   s Downloading    37%     8.42 MB/s up   1.10 MB/s peers tcp=24   utp=6    conn=3    seen=180   eta 3m 12s
```

- **`utp=`** é o critério do uTP. Não é inferido de configuração: é a contagem de
  peers vivos por `ConnectionKind::Utp`. Se ficar em `0` a corrida inteira com o
  uTP pedido, o uTP não está funcionando — e é isso que o resumo final diz.
- **`peak down`** contra a banda da conexão responde a saturação.
- O CPU se mede por fora, como nos outros spikes:

```powershell
$p = Start-Process .\target\release\spike-engine.exe -ArgumentList '<magnet>','--seconds','120' -PassThru
Start-Sleep 10; $p.Refresh(); $t0=$p.TotalProcessorTime; $w0=Get-Date
Start-Sleep 60; $p.Refresh()
"CPU de 1 nucleo: {0}%" -f [math]::Round((($p.TotalProcessorTime-$t0).TotalSeconds/((Get-Date)-$w0).TotalSeconds)*100,1)
```

Alvo: **≤ 15 % de um núcleo** saturando o link.

## Rodar as duas metades

O uTP só se prova por comparação. Duas corridas, mesmo torrent, mesma duração:

```powershell
cargo run --release -- <magnet> --seconds 180                # TCP + uTP
cargo run --release -- <magnet> --seconds 180 --tcp-only     # só TCP
```

O que se procura: taxa de pico parecida, `utp>0` na primeira, e — o motivo de o
uTP existir — **navegar na web enquanto semeia sem a página engasgar**. Essa
última é subjetiva e precisa de mão humana; é o ponto todo do LEDBAT.

## O que já se sabe sem rodar

Lido da fonte do librqbit 9.0.1, não da documentação — está tudo em
[`../../docs/spike-report.md`](../../docs/spike-report.md). O resumo:

| | |
|---|---|
| uTP | desligado por padrão (`ListenerMode::TcpOnly`), com um `TODO` do autor dizendo "once uTP is stable" |
| Porta | padrão é `0`, efêmera — mata toda conexão de entrada |
| UPnP | desligado por padrão |
| `Speed.mbps` | é **MiB/s**, não megabits. Ler como megabits erra por 8× |
| ETA | só existe como string formatada; calculamos o nosso |
| Erro | a mensagem vem num campo irmão do estado, fácil de descartar |
