# Security & Git Hook Gate

1. **Secrets & Identifier Hygiene**: Never hardcode API keys, JWTs, private keys, credentialed DB URLs, developer LAN IPs (`192.168.x.x`), static room/pairing codes (`xxx-xxx`), or personal device serials/models. Load from validated env/keystore or dynamic runtime discovery; keep `.env*` (except `.env.example`) out of Git.
2. **Injection Defense**: Parameterized SQL/ORM queries only; no shell string interpolation (`shell=True`) with untrusted input; sanitize untrusted HTML.
3. **Transport & Logging**: Keep TLS verification enabled in production; redact tokens, cookies, passwords, and PII from logs.
4. **Hook Integrity**: `.githooks/pre-commit`, `commit-msg`, and `pre-push` enforce secret scanning, Conventional Commits, Clean Arch boundaries, and file-size caps. **Never bypass hooks with `--no-verify`**.
