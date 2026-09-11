# App Mac no perfil Dev

Instale o engine e o ptyd seguindo `dev-local.md`. O app usa exclusivamente
o socket em `~/Library/Application Support/Soyeht.next/dev/engine.sock`.
Fechar a janela não fecha o shell. O botão de fechar uma pane pede confirmação.

Build sem assinatura, também usado na CI:

```sh
bash scripts/check_mac.sh
```

Build local com assinatura automática de desenvolvimento:

```sh
SOYEHT_DEVELOPMENT_TEAM=SEU_TIME bash scripts/check_mac.sh --sign
open '.cache/mac-derived/Build/Products/Debug/Soyeht Dev.app'
```

A conta e o certificado de desenvolvimento precisam estar configurados no
Xcode. Nenhum perfil ou certificado de distribuição é usado. O projeto é
`apps/mac/SoyehtMac.xcodeproj`, esquema `SoyehtMac`.

Os testes do cliente usam processos descartáveis e nunca o socket instalado:

```sh
(cd engine && cargo build --locked --workspace)
swift test --package-path apps/shared
```

Abra um terminal escolhendo uma pasta. A lista e a tela acompanham o engine.
Use “Adicionar iPhone” para exibir o QR; confira a impressão digital no telefone
antes de permitir acesso. O código expira em dez minutos. Parear exige que o
engine tenha um endereço da tailnet; loopback serve apenas aos testes locais.
