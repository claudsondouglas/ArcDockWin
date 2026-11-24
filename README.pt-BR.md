<div align="center">

# ArcDockWin

[English](README.md) · [繁體中文](README.zh-TW.md)

**Dock e barra de menus no estilo do macOS para o Windows 11, em Rust.**

Leve (um único `.exe`, sem runtime), nativo (Win32 + Direct2D + Composition) e feito para
substituir a barra de tarefas do Windows no dia a dia.

![ArcDockWin](docs/screenshots/desktop.png)

</div>

---

> **Projeto pessoal.** O ArcDockWin é um projeto de hobby que eu faço para o meu uso e compartilho caso seja útil
> para mais alguém. Não é um produto comercial: não há suporte nem garantia, e as coisas podem mudar ou quebrar entre
> versões. Issues e pull requests são bem-vindos, mas respondo no meu tempo.

## ✨ Recursos

### Dock
- Aparece no monitor principal por padrão; dá para mostrar em **todos os monitores** em Configurações → Dock.
- Vidro desfocado de verdade (o que está atrás aparece borrado), cantos arredondados e sombra suave.
- Ampliação dos ícones ao passar o mouse e pontinho embaixo dos apps abertos.
- **Ícone pulando** quando um app pede atenção, com um pontinho vermelho até você abrir o app.
- **Arrastar para reordenar**: solte um app aberto entre os fixados para fixar, ou arraste para cima para desafixar.
  O lançador fica sempre como último app.
- **Lixeira e Downloads** no fim da dock, com painel de arquivos recentes.
- Esconde sozinha quando uma janela encosta nela; com o ocultar automático ligado, levar o mouse a qualquer ponto
  da borda de baixo traz ela de volta. Fica visível enquanto o painel de Downloads está aberto, e clicar de novo na
  pilha fecha o painel.
- Apps que rodam dentro do WebView2 (como o WhatsApp) mostram o próprio ícone, e não o do Edge WebView.
- Clicar num app cuja única janela está minimizada restaura a janela.
- Segue o **Modo Escuro/Claro** do Windows.

### Animação ao minimizar
As janelas encolhem até o ícone do app na dock com efeito **Gênio** ou **Escala**, sumindo aos poucos conforme
chegam perto do ícone, a 60 fps. Funciona ao clicar no botão de minimizar da janela, no atalho **Win+↓** e ao clicar
no ícone do app na dock para minimizá-lo. Também funciona nas pastas do Explorador do Windows 11 (as com abas).

Para não ter animação dobrada, o ArcDockWin desliga a animação nativa de minimizar do Windows só durante a dele. O
valor original volta logo depois, quando o ArcDockWin é fechado e também pelo vigia, se a dock travar.

### Barra do topo
- Aparece em **todos os monitores** por padrão; dá para limitar ao principal em Configurações → Barra do topo.
- **Assume a cor do app em foco** (10% mais escura, para separar a barra do app), na hora em que você troca de janela.
- Montada em **módulos**: CPU/RAM, Relógio, Bandeja, Central de Controle e os opcionais de uso do Claude e do Codex.
  Cada um pode ser desligado, ir para a zona da esquerda, do centro ou da direita e mudar de ordem dentro da zona.
- Relógio com calendário ao clicar; 24 h ou 12 h, segundos e data opcionais.
- Ícones da bandeja com painel dos ocultos e escolha de quais ficam na barra. Quando falta espaço, os ícones se
  recolhem sozinhos para a seta (sem mudar a sua escolha) e o CPU/RAM é o último a sumir.
- O item com o painel aberto fica destacado.
- Os menus dos apps da bandeja abrem junto do ícone na barra do topo, e o duplo clique num ícone do painel dos
  ocultos funciona como na barra de tarefas do Windows.
- Se o Explorer reiniciar, a barra do topo reserva o espaço dela de novo, e janelas maximizadas não ficam por baixo.

### Painel Desempenho
Clique no CPU/RAM da barra do topo para ver o uso de CPU, memória e GPU (com a memória de vídeo) e as temperaturas
de CPU e GPU, com um botão para abrir o Gerenciador de Tarefas. GPU e temperatura só são lidas com o painel aberto.

As temperaturas dependem do hardware: a da CPU vem da biblioteca da AMD e só funciona em APUs Ryzen; a da GPU
funciona em placas Radeon (AMD) e NVIDIA. O que não dá para ler aparece como "—".

![Painel Desempenho](docs/screenshots/performance.png)

### Uso do Claude e do Codex
Dois módulos opcionais da barra do topo, desligados por padrão (ligue em Configurações → Barra do topo), para quem usa
o [Claude Code](https://claude.com/claude-code) ou o [Codex](https://openai.com/codex). A barra mostra quanto da
sessão de 5 horas já foi usado, e o clique abre um painel com:
- **Limites**: a sessão de 5 horas e os limites semanais, com o tempo que falta para cada um zerar.
- **Tokens por dia** nos últimos 7 dias e **tokens por modelo**, lidos dos registros locais das sessões.
- Um link para a página de uso no claude.ai ou no chatgpt.com.

Os limites vêm do mesmo serviço que os apps oficiais consultam, com o login que eles já salvaram no PC
(`~/.claude/.credentials.json` e `~/.codex/auth.json`). O ArcDockWin nunca renova esse login: quando ele vence, o
painel pede para abrir o app. Os números são atualizados a cada 5 minutos.

| Claude | Codex |
| :---: | :---: |
| ![Painel de uso do Claude](docs/screenshots/usage-claude.png) | ![Painel de uso do Codex](docs/screenshots/usage-codex.png) |

### Central de Controle
No visual do macOS Tahoe, com cartões arredondados:
- Wi‑Fi / Rede, Bluetooth, Modo Escuro e Energia (bateria, desligar, reiniciar…).
- **Tocando agora** (capa, título e botões de qualquer player que fale com o Windows: Spotify, navegador…).
- **Brilho** por DDC/CI do monitor onde está a barra que você clicou, e **volume**.

### Configurações
Botão direito num espaço vazio da dock → **Configurações…** Abre como uma janela normal (barra de título, Alt+Tab,
ícone na dock), com fundo Mica que segue o tema claro/escuro do Windows. As mudanças valem na hora.

| Página | Opções |
| --- | --- |
| Dock | Tamanho dos ícones (32–72 px), ampliação ao passar o mouse e o tamanho dela (1,1×–2,0×), ocultar automaticamente, mostrar Downloads e Lixeira, mostrar em todos os monitores |
| Animações | Animação ao minimizar ligada/desligada, efeito Gênio ou Escala |
| Barra do topo | Prévia da barra; cada módulo ligado/desligado, zona (Esquerda/Centro/Direita) e ordem (▲▼); relógio (24 h, segundos, data); mostrar em todos os monitores |
| Geral | Iniciar com o Windows; Sobre |

| Página Dock (tema claro) | Página Barra do topo (tema escuro) |
| :---: | :---: |
| ![Configurações, página Dock](docs/screenshots/settings-dock.png) | ![Configurações, página Barra do topo](docs/screenshots/settings-topbar.png) |

A mesma barra com o relógio na esquerda e o CPU/RAM na direita:

![Barra do topo com os módulos reorganizados](docs/screenshots/topbar-modules.png)

### Idiomas
O ArcDockWin segue o idioma do Windows e já vem em português (Brasil), inglês e chinês tradicional (Taiwan). Windows
em português fica em português, em chinês fica em chinês tradicional, e qualquer outro idioma cai no inglês. O
idioma é verificado enquanto o app roda, então trocar não exige reiniciar. As tabelas de tradução estão em
[`locales/en.tsv`](locales/en.tsv) e [`locales/zh-TW.tsv`](locales/zh-TW.tsv). Outros idiomas são bem-vindos por
pull request.

### Segurança
- A barra de tarefas do Windows fica escondida enquanto o ArcDockWin roda, inclusive quando o Windows tenta
  mostrá-la de novo (ao tirar print, por exemplo).
- Um processo **vigia** restaura a barra do Windows se o ArcDockWin travar ou for encerrado à força.

## 📸 Prints

| Central de Controle | Dock |
| :---: | :---: |
| ![Central de Controle](docs/screenshots/central.png) | ![Dock](docs/screenshots/dock.png) |

| Barra do topo | Lançador |
| :---: | :---: |
| ![Barra do topo](docs/screenshots/topbar.png) | ![Lançador](docs/screenshots/launcher.png) |

## 🚀 Instalação

### Baixar pronto

1. Baixe o `arcdockwin.exe` da [última versão](https://github.com/claudsondouglas/ArcDockWin/releases/latest)
   (ou o `.zip`, que já vem com os scripts de ícones).
2. Coloque numa pasta fixa (por exemplo `C:\Programas\ArcDockWin`) e abra.

> O `.exe` não é assinado digitalmente, então o Windows pode mostrar **"O Windows protegeu o computador"**.
> Clique em **Mais informações → Executar assim mesmo**.

### Atualizações

O ArcDockWin procura versões novas no GitHub sozinho. Quando sai uma, aparece **Atualizar o ArcDockWin** no menu
do botão direito da dock e em Configurações → Geral → Sobre. Um clique baixa a versão nova, confere o arquivo e
reabre o app. Nada é instalado sem você pedir, e dá para desligar a busca em Configurações → Geral.

Requisito: **Windows 11**. O Windows 10 não tem suporte oficial, mas há relatos de que funciona; nele, a barra de menus e o painel de controle usam a fonte de ícones Segoe MDL2 Assets.

### Compilar do código

Requisitos: **Windows 11** e o [Rust](https://rustup.rs) (toolchain MSVC).

```powershell
git clone https://github.com/claudsondouglas/ArcDockWin.git
cd ArcDockWin
cargo build --release
.\target\release\arcdockwin.exe
```

Na primeira execução o ArcDockWin se registra para **iniciar com o Windows** (em `HKCU\...\Run`), apontando para o
`.exe` no lugar onde ele está: por isso, não deixe ele na pasta Downloads. Dá para desligar em Configurações → Geral.

### Como sair
Clique com o botão direito num espaço vazio da dock → **Sair do ArcDockWin**. A barra de tarefas do Windows volta
na hora.

> Se algo der errado e a barra do Windows não voltar, abra o ArcDockWin de novo e saia pelo menu, ou reinicie o
> Explorer (Gerenciador de Tarefas → Windows Explorer → Reiniciar).

### Configuração
Tudo fica em `%APPDATA%\ArcDockWin\`:

| Arquivo | O que guarda |
| --- | --- |
| `pinned.txt` | Apps fixados na dock, um caminho por linha (pode editar à mão). |
| `icons.txt` | Ícones trocados por app (`nome.exe=C:\caminho\icone.ico`), veja abaixo. |
| `colors.db` | Cor de cada app na barra do topo e quais ícones da bandeja ficam visíveis (SQLite). |
| `settings.txt` | Opções da janela de Configurações (`chave=valor`, ex.: `icon_size=48`, `mod.clock=center,0,on`). Chave ausente usa o padrão. `tray_color=1` (ainda sem opção na janela) mantém as cores dos apps nos ícones da bandeja em vez de cinza. |
| `animation.txt` | Animação ao minimizar: `enabled=1` ou `0`, `effect=genie` ou `scale`. |

### Limitações conhecidas
- Apps que desenham a própria barra de título (sem o botão de minimizar padrão) são detectados por heurística, então
  clicar no "_" deles pode minimizar sem animação. Win+↓ e o ícone da dock continuam animando.
- As temperaturas dependem do hardware e dos drivers (veja o painel Desempenho acima).
- Numa barra muito estreita, os ícones da bandeja e depois o CPU/RAM são escondidos para caber.

## 🎨 Ícones do macOS (MacTahoe)

A dock usa o ícone do próprio app por padrão. Para deixar tudo com cara de Mac, a pasta [`icons/`](icons) tem dois
scripts que aplicam o tema [**MacTahoe**](https://github.com/vinceliuice/MacTahoe-icon-theme) de
[vinceliuice](https://github.com/vinceliuice) na dock **e no Windows inteiro**: atalhos da área de trabalho e do menu
Iniciar, pastas comuns e conhecidas do Explorer (Área de Trabalho, Documentos, Downloads, Imagens, Música e Vídeos),
discos, Lixeira e Este Computador. O script usa os pontos de personalização do Windows Shell e salva um backup para
restaurar as associações originais.

![Ícones MacTahoe](docs/screenshots/icones.png)

### 1. Instale o ImageMagick

```powershell
winget install ImageMagick.ImageMagick
```

Depois **feche e abra o terminal** para o comando `magick` passar a existir.

### 2. Baixe e converta os ícones

```powershell
powershell -ExecutionPolicy Bypass -File .\icons\Converter.ps1
```

O script baixa o tema, converte os ~1400 ícones de SVG para `.ico` (leva cerca de 1 minuto) e monta tudo em
`C:\Icones\MacTahoe`:

```
C:\Icones\MacTahoe\
├── apps\          ícones de apps (google-chrome.ico, spotify.ico, …)
├── sistema\       pastas, Lixeira, discos e Este Computador
├── aliases.txt    nomes alternativos de cada ícone
└── IconesTahoe.ps1
```

> Use essa pasta: os ícones da Lixeira e de Downloads da dock são lidos de `C:\Icones\MacTahoe\sistema`.

### 3. Aplique

```powershell
cd C:\Icones\MacTahoe
powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Testar     # só mostra qual ícone cada atalho vai ganhar
powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Instalar   # aplica e fica na bandeja
```

O `-Instalar` pede permissão de administrador (pastas e discos são configurações do sistema) e cria uma tarefa
agendada que abre o **Ícones Tahoe** na bandeja a cada logon. Ele reaplica os ícones de tempos em tempos, para pegar
apps novos e atalhos que as atualizações recriam. Pelo menu da bandeja dá para:

- **Aplicar agora** (ou dar dois cliques no ícone);
- escolher o intervalo em **Aplicar automaticamente** (1 h a 24 h, ou desligado);
- **Restaurar ícones do Windows**.

A dock recebe os ícones pelo arquivo `%APPDATA%\ArcDockWin\icons.txt`, que o script escreve. O botão Iniciar ganha
o ícone de grade de apps.

### Ícone errado ou faltando?

O tema é feito para Linux, então o script procura o ícone pelo nome do `.exe`, pelo nome do atalho e pelos aliases do
tema. Quando o nome do programa no Windows não bate, adicione uma linha na tabela `$Extra` no começo do
`IconesTahoe.ps1`:

```powershell
$Extra = @{
  msedge = 'microsoft-edge'   # nome do .exe (sem .exe) = nome do ícone em apps\ (sem .ico)
  code   = 'visual-studio-code'
  # ...
}
```

Para trocar o ícone de um único app na dock, sem script, edite `%APPDATA%\ArcDockWin\icons.txt`:

```
spotify.exe=C:\Icones\MacTahoe\apps\spotify.ico
```

O **Ícones Tahoe** reescreve o `icons.txt` toda vez que aplica os ícones. Para manter os seus ajustes, coloque as
mesmas linhas em `C:\Icones\MacTahoe\icons-custom.txt`: elas têm prioridade sobre o tema e entram em toda aplicação.

### Desfazer

```powershell
powershell -ExecutionPolicy Bypass -File C:\Icones\MacTahoe\IconesTahoe.ps1 -Desinstalar
```

Isso volta todos os ícones originais (o script guarda um backup do que trocou) e remove a tarefa agendada.

## 🛠️ Como funciona

| Parte | Tecnologia |
| --- | --- |
| Desenho | [tiny-skia](https://github.com/RazrFalcon/tiny-skia) + Direct2D/DirectWrite, janelas em camadas (`UpdateLayeredWindow`) |
| Vidro | `Windows.UI.Composition` (backdrop do DWM) |
| Reserva de espaço | AppBar (`SHAppBarMessage`) |
| Janelas e foco | `SetWinEventHook` e `RegisterShellHookWindow` |
| Mídia | `Windows.Media.Control` (GSMTC) |
| Brilho | DDC/CI (`dxva2`) |
| Cores e bandeja | SQLite do próprio Windows (`winsqlite3.dll`) |
| Animação ao minimizar | Ganchos de mouse e teclado de baixo nível para capturar a janela antes de minimizar (sem injetar DLL) |
| Desempenho | Contadores PDH, DXGI, ADL da AMD (`atiadlxx.dll`) e NVML da NVIDIA (`nvml.dll`), carregados só com o painel aberto |

## 🙏 Créditos

- Ícones: [MacTahoe icon theme](https://github.com/vinceliuice/MacTahoe-icon-theme) por vinceliuice (GPL-3.0). Eles não
  vêm neste repositório: o `Converter.ps1` baixa direto do projeto original.
- Tradução para chinês tradicional e ícones das pastas do Explorer: [popuchoco](https://github.com/popuchoco)
  ([#1](https://github.com/claudsondouglas/ArcDockWin/pull/1)). Obrigado!
- Ideia da rede de segurança da barra de tarefas: [Lemxy/WinRust](https://github.com/Lemxy/WinRust).
- Inspirado no macOS. Não é afiliado à Apple.

## 📄 Licença

[MIT](LICENSE). Os ícones MacTahoe, baixados à parte pelo `Converter.ps1`, seguem a licença GPL-3.0 do projeto original.
