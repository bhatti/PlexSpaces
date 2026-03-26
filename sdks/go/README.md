# PlexSpaces Go SDK

TinyGo-oriented SDK for PlexSpaces WASM actors: implement the `Actor` interface, embed `BaseActor`, and use generated WIT imports for host calls.

## Module layout

- **`plexspaces/`** — Actor API, host wrappers, router
- **`plexspaces/proto/`** — Generated Go types from Protocol Buffers (`make proto` / `make proto-go` at repo root)

Structured errors from the host can be parsed with **`HostError.ParseErrorDetail()`** when the message contains JSON aligned with `ErrorDetail`.

## Documentation

- [docs/sdk.md](../../docs/sdk.md) — Go SDK section and cross-language notes
- [docs/polyglot.md](../../docs/polyglot.md) — WASM build and WIT

## Tests

From the repository root:

```bash
make test   # includes go test ./... under sdks/go
```

Or locally:

```bash
cd sdks/go && go test ./...
```

## License

LGPL-2.1-or-later
