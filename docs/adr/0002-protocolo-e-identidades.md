# 0002 — Formatos e identidades da primeira fatia

Estado: aceito para implementação, 2026-09-10.

## Contexto

O 08 fixa envelopes, limites e operações, deixando ao esquema os tipos
concretos. A primeira fatia não implementa ACP, tarefas nem push.

## Decisão

JSON Schema 2020-12 em subconjunto fechado e documentado. Definições de
objetos, enums, arrays, referências e escalares geram modelos serde, Codable
e TypeScript. O catálogo associa cada nome RPC ao tipo de parâmetros e
resultado. Gerador não busca arquivos nem executa comandos indicados pelo
esquema. UTF-8, JSON sem números não finitos; u64 transmitido como decimal.

Ids de objeto são UUIDs opacos. Datas usam RFC3339 UTC. Byte strings JSON
usam base64 padrão com padding. Dados binários do terminal nunca passam
por base64. Inteiros dos cabeçalhos são big-endian. `eventId` identifica
eventos duráveis; desafios e partes de snapshot são notificações efêmeras,
sem sequência de banco. `hello` recebe uma única resposta result welcome.

O texto assinado no hello concatena UTF-8 de `soyeht-hello-v1`, nonce
decodificado de 32 bytes, engineId UUID canônico UTF-8 (36 bytes), e token
UTF-8. Código de pareamento é segredo aleatório em base64url de 32 bytes;
o HMAC usa os bytes UTF-8 desse código seguidos da chave pública bruta para
derivar a chave SHA-256, e nonce bruto + deviceName UTF-8 como mensagem.
Todas as assinaturas e provas transmitidas usam base64 padrão. Código
consumido e pedido de pareamento são gravados atomicamente com o dispositivo.

## Motivo

Bytes assinados e campos ausentes não podem depender da linguagem do
cliente. Definir essas escolhas uma vez permite vetores interoperáveis e
evita manter três protocolos manuais. O padrão do 08 continua preservado.
