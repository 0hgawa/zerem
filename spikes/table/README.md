# Spike S0.1 + S0.3 — tabela virtualizada e tick por snapshot

Código descartável da Fase 0. Existe para responder duas perguntas antes de haver
arquitetura apoiada nelas: **o Slint aguenta a tabela?** e **um tick custa o que
mudou ou o que está na tela?**

Resultados e decisões: [`../../docs/spike-report.md`](../../docs/spike-report.md).

## Rodar

```powershell
cargo run --release
```

O console imprime uma linha por tick — é dela que sai o relatório:

```
rows=2000   moved=26    changed=26    sort=skip     diff=92   us frames=13   reset=false
```

`moved` é o que a sessão mexeu, `changed` é o que o diff sujou. **Os dois têm de
bater**; divergir significa que o modelo está repintando linha que não mudou.
`reset=true` num tick comum significa que o scroll e a seleção foram perdidos.

## Teclas

| Tecla | Efeito |
|---|---|
| `S` | stress — repinta o mais rápido possível, para achar o teto de fps |
| `C` | churn — toda linha muda a cada tick, o pior caso |
| `T` | alterna tema claro/escuro |
| `1` `2` `3` | 200 / 2 000 / 10 000 linhas |

Clique no cabeçalho ordena, clique de novo inverte. A alça na borda direita de
cada cabeçalho redimensiona a coluna. Clique seleciona, `Ctrl` alterna, `Shift`
seleciona intervalo.

## Variáveis de ambiente

Existem para medir sem mão no teclado.

| Variável | Padrão | Para quê |
|---|---|---|
| `ZEREM_ROWS` | 2000 | tamanho da lista |
| `ZEREM_TICK_MS` | 1000 | período do tick; `60000` isola o custo da janela parada |
| `ZEREM_SORT_COL` | 0 | coluna inicial; `4` começa numa coluna volátil, que é o caminho caro |
| `ZEREM_CHURN` | 0 | `1` começa no pior caso |
| `SLINT_BACKEND` | `winit-femtovg` | `winit-software` para o caminho de repintura parcial |

O spike mantém o femtovg como padrão porque é o único que fornece o contador de
quadros. **O app usará o de software** — a decisão está no relatório, com os
números que a sustentam.

## A verificação que falta

O relatório fecha tudo menos uma coisa, que exige mão no mouse: **rolagem**. A
repintura parcial do renderizador por software não ajuda quando a viewport
inteira se move.

```powershell
$env:ZEREM_ROWS=2000
$env:SLINT_BACKEND="winit-software"; cargo run --release
$env:SLINT_BACKEND="winit-femtovg";  cargo run --release
```

Rolar a lista de ponta a ponta nos dois e comparar. O que se procura é rasgo ou
engasgo visível — se o software segurar, ele fica como padrão e o app inteiro
custa 17 MB em vez de 120 MB.

> O contador de `frames` só funciona no femtovg: o *rendering notifier* não
> existe no renderizador por software. No modo software ele imprime `0` e avisa
> uma vez no stderr.
