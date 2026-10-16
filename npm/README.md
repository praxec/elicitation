# `@matthew-cochran/elicitation`

The `npx` launcher for the [`elicitation`](https://github.com/praxec/elicitation)
MCP server. On first run it downloads the release binary for your platform,
verifies it against the release's `checksums.sha256`, caches it, and execs it
with stdin/stdout/stderr passed through — so stdout stays the MCP stdio channel.

```sh
npx -y @matthew-cochran/elicitation
```

`elicitation` is an MCP server, not an interactive command: point your MCP client
at it (`{ "command": "npx", "args": ["-y", "@matthew-cochran/elicitation"] }`).
The CLI also takes `--version`, `--help`, and
`elicitation skills install --target <claude|codex|both> --dir <path>`.

The package version matches the binary release version; `<version>` downloads
`elicitation-<triple>` from the `v<version>` release.

## Environment variables

| Variable | Effect |
|----------|--------|
| `ELICITATION_BINARY` | Use this pre-installed binary and skip the cache/download. |
| `ELICITATION_CACHE_DIR` | Cache root (default: `$XDG_CACHE_HOME`, else `~/.cache`; `%LOCALAPPDATA%` on Windows). |
| `ELICITATION_DOWNLOAD_BASE` | Release base URL (default: `https://github.com/praxec/elicitation/releases`). |
| `ELICITATION_SKIP_DOWNLOAD=1` | Skip the install-time pre-fetch. |
| `PRAXEC_ALLOW_INSECURE=1` | Allow an `http://` download base (local testing only). |

Unsupported platforms and CPU architectures are refused with a clear error; the
launcher never falls back to an unverified binary. A cached binary whose sha256
no longer matches the digest recorded at install time is refused.

## License

[Apache-2.0](../LICENSE).
