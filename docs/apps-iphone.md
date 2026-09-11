# App iPhone no perfil Dev

Build para Simulator, igual à CI:

```sh
bash scripts/check_ios.sh
```

Para executar no Simulator com acesso ao Keychain, use assinatura local:

```sh
SOYEHT_DEVELOPMENT_TEAM=SEU_TIME bash scripts/check_ios.sh --sign-simulator
```

O build sem assinatura serve ao check de compilação da CI. Ele não oferece a
identidade exigida pelo Keychain para executar o pareamento.

Build local para aparelho com assinatura automática de desenvolvimento:

```sh
SOYEHT_DEVELOPMENT_TEAM=SEU_TIME bash scripts/check_ios.sh --sign
```

Abra `apps/ios/SoyehtPhone.xcodeproj`, esquema `SoyehtPhone`, selecione o seu
iPhone e execute com a conta de desenvolvimento do time. O bundle é
`com.soyeht.next.dev.ios`. O app atual não participa desse build.

No Mac novo, escolha “Adicionar iPhone”. No iPhone novo, escolha “Ler QR do Mac”.
Compare as impressões digitais e permita no Mac. Abra uma pane pela lista.
O iPhone lê o terminal; teclado ao vivo fica fora desta fatia. Os dois aparelhos
precisam estar conectados à mesma tailnet.

Ao voltar do fundo, aguarde a sincronização completa. Se o acesso foi revogado,
faça novo pareamento. “Esquecer este Mac” remove a credencial do iPhone; revogar
no Mac encerra o acesso no engine e fecha conexões já abertas.

Sem câmera compatível, o mesmo link pode ser colado ou aberto no app. O QR
expira em dez minutos e só uma aprovação consome o código. Nunca coloque o link,
o token ou a chave privada em um relatório ou issue.
