# Executable protocol

`protocol.json` is the single source of formats. `x-methods` binds each method
to its parameters and result. `x-events` binds each notification to its parameters
and declares whether it has a durable cursor. `x-limits` defines shared limits;
`x-framing` describes binary frame integers.

Methods whose parameters declare `paneId` use it as their operation-log route.
`x-paneRoute` names a differently spelled singular pane identifier.
`x-paneNonRouteIds` lists every other parameter name whose type is `Id` or an
array of `Id`. Generation fails when an identifier name is neither the route
nor explicitly classified, and also fails when a listed name becomes unused.

`profiles.json` is the single source for installed profile values. It defines
the Dev data directory, socket names, TCP port and launchd labels. The same
generator emits constants for Rust, Swift and Python, plus the launchd service
templates. Runtime code and installation scripts consume those generated values.

The format is JSON Schema 2020-12. The generator accepts local references in
`$defs`, objects with required and optional fields, arrays, strings, numbers,
integers, booleans, null, enum and oneOf. It accepts numeric and length limits,
pattern and contentEncoding declared in the schema.
Any unknown keyword in a type stops generation.

Readers ignore unknown fields, as required by contract 08. Enums remain closed.
Generated models represent types; state rules, identity, authorization and transport
limits belong to the engine. The TypeScript reader validates parameters against
the schema before returning types. Serde and Codable check types during deserialization.
Frame size limits are checked by all three binary codecs.

Requests and notifications use the JSON-RPC 2.0 envelope. `welcome` is the result
of `hello`. Challenges, snapshot parts, stream events, local-server changes,
web console counts and web driving state are notifications without a state
cursor; durable events carry `params.eventId`. 64-bit integers travel as decimal
strings; in binary frames they are big-endian u64.

To change a format, edit the schema and corresponding vectors:

```sh
python3 contracts/generate.py
```

The generator does not use the network. It produces Rust, Swift and TypeScript
models, profile constants, limits, service templates and test dispatchers.
Generated files are included in the same commit as their source. `--check`
writes nothing and fails if they differ.

`vectors/messages.json` contains type values and full envelopes for every method
and event. The same values pass through Rust and Swift types and the TypeScript
reader. `vectors/frames.json` fixes reference bytes in both directions. Negative
controls check truncation, excessive size, invalid frame kinds, incorrect JSON
types and unknown enums.
