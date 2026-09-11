# Rodar o perfil Dev novo

Instale as ferramentas de `docs/toolchains.md`. Compile na raiz:

```sh
cargo build --manifest-path engine/Cargo.toml --locked --workspace
python3 scripts/install_dev.py
engine/target/debug/soyeht --profile dev status
engine/target/debug/soyeht --profile dev pane create --shell
engine/target/debug/soyeht --profile dev pane list
```

O instalador cria somente os serviços `com.soyeht.next.dev.ptyd` e
`com.soyeht.next.dev.engine`. Os executáveis, o banco e os logs ficam em
`~/Library/Application Support/Soyeht.next/dev/`. Os arquivos dos serviços
ficam em `~/Library/LaunchAgents/`. O engine escuta no loopback e no endereço
da tailnet (IPv4 e IPv6), na porta 8291. Inicie a tailnet antes da instalação.

`pane create --shell --cwd CAMINHO` abre o shell nessa pasta e cria o
workspace correspondente quando necessário. O diretório atual é o padrão.
Para repetir uma criação com a mesma identidade, use
`--idempotency-key CHAVE`. As respostas são JSON; erros saem em stderr com
código de saída diferente de zero.

```sh
engine/target/debug/soyeht --profile dev pane close ID
engine/target/debug/soyeht --profile dev device pair-start
engine/target/debug/soyeht --profile dev device list
```

O pareamento ainda precisa do consentimento no cliente Mac. Exibir o código
pela CLI não autoriza um aparelho sozinho.

Para substituir os binários do perfil novo, pare primeiro esses dois
serviços. Isso encerra os shells desse perfil. O instalador recusa substituir
um serviço que continua carregado.

```sh
launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.engine"
launchctl bootout "gui/$(id -u)/com.soyeht.next.dev.ptyd"
cargo build --manifest-path engine/Cargo.toml --locked --workspace
python3 scripts/install_dev.py
```

Para atualizar apenas o engine e preservar os shells, pare somente o
serviço `com.soyeht.next.dev.engine`, compile e rode
`python3 scripts/install_dev.py --component engine`. O supervisor permanece
ativo e o engine adota os mesmos processos.

Os testes podem usar `--data-dir CAMINHO` para conectar a uma instalação
temporária. Em um runner sem domínio gráfico, o instalador aceita
`--domain user`; os comandos de inspeção e parada usam esse mesmo domínio.
Nenhuma dessas opções aceita outro nome de perfil ou de serviço.
