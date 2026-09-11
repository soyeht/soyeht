# Protocolo executável

`protocol.json` é a fonte única dos formatos. `x-methods` liga cada método
a seus parâmetros e resultado. `x-events` liga cada notificação a seus
parâmetros e declara se ela tem cursor durável. `x-limits` define os limites
compartilhados; `x-framing` descreve os inteiros do quadro binário.

O formato é JSON Schema 2020-12. O gerador aceita referências locais em
`$defs`, objetos com campos obrigatórios e opcionais, arrays, strings,
números, inteiros, booleanos, null, enum e oneOf. Aceita os limites numéricos
e de comprimento, pattern e contentEncoding declarados no esquema.
Qualquer palavra-chave desconhecida em um tipo interrompe a geração.

Leitores ignoram campos desconhecidos, conforme o contrato 08. Enums
permanecem fechados. Os modelos gerados representam os tipos; regras de
estado, identidade, autorização e limites do transporte pertencem ao engine.
O leitor TypeScript valida os parâmetros contra o esquema antes de entregar
os tipos. Serde e Codable verificam os tipos durante a desserialização.
Os limites de tamanho do quadro são verificados pelos três codecs binários.

Pedidos e notificações usam o envelope JSON-RPC 2.0. `welcome` é o resultado
de `hello`. Desafio, partes do snapshot e eventos do stream são notificações
sem cursor de estado; eventos duráveis levam `params.eventId`. Inteiros de
64 bits vão como string decimal; no quadro binário vão como u64 big-endian.

Para alterar um formato, edite o esquema e os vetores correspondentes:

```sh
python3 contracts/generate.py
```

O gerador não usa rede. Ele produz os modelos Rust, Swift e TypeScript, os
limites e os despachantes dos testes. Os arquivos produzidos entram no mesmo
commit do esquema. `--check` não escreve nada e falha quando divergem.

`vectors/messages.json` contém valores dos tipos e envelopes completos para
cada método e evento. Os mesmos valores passam pelos tipos Rust e Swift e
pelo leitor TypeScript. `vectors/frames.json` fixa bytes de referência nas
duas direções. Controles negativos verificam truncamento, tamanho excessivo,
tipo de quadro inválido, tipos JSON incorretos e enums desconhecidos.
