# Zerem — o que foi medido

O encerramento da Fase 3: a tabela de alvos de aceite, aferida no binário que
existe hoje, e **dito com todas as letras o que não foi aferido**. Um relatório
de desempenho que preenche as lacunas por extrapolação é pior que um relatório
com lacunas, porque a lacuna pelo menos se vê.

Números da Fase 0 estão em [spike-report.md](spike-report.md) e não são
repetidos aqui como se fossem novos. Eles mediram protótipos descartáveis; estes
medem o app.

## A máquina

| | |
|---|---|
| CPU | AMD Ryzen 9 7900 — 12 núcleos, 24 threads |
| RAM | 31 GB |
| Sistema | Windows 11 IoT Enterprise LTSC |
| Build | `cargo build --release`, `opt-level = "s"`, LTO, `codegen-units = 1` |
| Estado | 1 torrent restaurado da sessão, baixando a ~85 KB/s |

**Um torrent, não mil.** É o que havia na sessão de verdade no momento da
medição, e forjar novecentos e noventa e nove para preencher uma linha da tabela
mediria outra coisa. As linhas que precisam de uma lista grande estão marcadas
como não aferidas, abaixo.

## Aferido

| Métrica | Alvo | Medido | Como |
|---|---|---|---|
| Binário (exe, sem instalador) | ≤ 16 MB | **15,37 MB** | tamanho do arquivo |
| Cold start até a janela existir | ≤ 300 ms | **81 – 95 ms**, mediana **92** | 5 execuções, cronômetro do `Start-Process` até `MainWindowHandle ≠ 0` |
| CPU, janela aberta, baixando | ≤ 0,5 % ocioso | **0,156 % de um núcleo** | delta de `Process.CPU` sobre 40 s |
| Working set, sessão viva | ≤ 60 MB | **57,6 MB** | `WorkingSet64`, mesma janela |
| Custo do tick | ≤ 1 ms | mediana **1 µs**, p95 **60 µs**, máx **71 µs** | 46 tiques do log com `ZEREM_LOG=debug` |

### O que cada número quer dizer

**Cold start.** O cronômetro para quando a janela *existe*, que é um instante
antes de ela estar pintada — então o número real é um pouco maior que 92 ms.
Como o alvo é 300 ms e a margem é de mais de três vezes, refinar a medição não
mudaria o veredito. Vale dizer que essas execuções restauram **um** torrent da
resume data; uma sessão com centenas paga mais.

**CPU.** 0,0625 s de CPU em 40 s de relógio. Sobre um núcleo dá 0,156 %; sobre a
máquina inteira, 0,0065 %. O alvo de 0,5 % é para o app **ocioso**, e esta
medição foi tirada com o torrent baixando — ou seja, **fazendo mais trabalho que
o caso do alvo**. Ocioso é limitado por cima por isso, então o alvo está batido
com folga mesmo sem a medição isolada.

De quebra, valida o alvo de CPU baixando: a 0,085 MB/s, o teto de 1,5 % por MB/s
prevê 0,13 %; o medido é 0,156 %. Na mesma ordem de grandeza, com um torrent só
— não é confirmação do alvo a 1 Gbps, é consistência.

**Tick.** A mediana de 1 µs não é o custo de formatar uma linha: é o custo de um
tique em que **nada mudou**, e o modelo compara e sai. O p95 de 60 µs é o tique
que de fato reformatou a linha. É a zona morta das taxas
([`rate.rs`](../crates/zerem-core/src/rate.rs)) aparecendo no orçamento: uma taxa
que não muda deixa a linha idêntica byte a byte, e o diff pula.

## Não aferido, e por quê

| Métrica | Alvo | Situação |
|---|---|---|
| RAM — 1 000 torrents na lista | ≤ 80 MB WS | Precisa de mil torrents reais. A Fase 0 mediu **2 000 linhas sintéticas a 31,9 MB**, mas aquilo era o modelo e o renderizador; mil torrents no librqbit trazem mil handles, mil estados e mil listas de arquivos, que é outra conta. |
| RAM — semeando 20 ativos | ≤ 100 MB WS | Idem: precisa de vinte torrents completos e de gente puxando deles. |
| CPU semeando | ≤ 1 % de um núcleo | A Fase 0 mediu 0,77 % na spike da engine. Não repetido no app. |
| CPU ocioso na bandeja | ≤ 0,1 % | A Fase 0 mediu 0,000 % com o tique parado, que é o mecanismo que continua no app — mas esconder a janela para medir significa mexer na sessão de quem está usando. |
| Custo do tick, 2 000 torrents | ≤ 1 ms | A Fase 0 mediu 92 µs no spike da tabela. Aqui há uma linha. |
| Scroll com 2 000 torrents | 60 fps | Verificação humana, ainda pendente desde a Fase 0. |
| Resposta de qualquer clique | < 100 ms | Atualização otimista por construção: todo comando aplica na hora e o snapshot seguinte é a verdade. Não é um número, é uma propriedade do desenho. |

## Como refazer

```powershell
cargo build --release
(Get-Item target/release/zerem.exe).Length / 1MB     # binário

$p = Get-Process zerem
$a = $p.CPU; Start-Sleep 40; $p.Refresh()
($p.CPU - $a) / 40 * 100                             # % de um núcleo
$p.WorkingSet64 / 1MB                                # working set
```

```bash
ZEREM_LOG=debug ./target/release/zerem.exe 2>&1 \
  | sed 's/\x1b\[[0-9;]*m//g' | grep -oE "micros=[0-9]+"
```

A partir da Fase 4 isto vira regressão no CI — criterion sobre o limitador, o
diff do modelo e a ordenação, mais uma checagem de RAM ociosa. Sem isso, "10 em
performance" volta a ser opinião em três meses.
