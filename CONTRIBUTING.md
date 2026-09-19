# Contributing to ShadowCode

Thanks for helping improve the harness.

## Setup

```bash
git clone https://github.com/ShadowfetchLinux/ShadowCode.git
cd ShadowCode
python3 -m pip install -e ".[dev]" --user
cd ui && npm install && npm run build && cd ..
python3 -m pytest
```

Use the mock provider for tests. Do not commit API keys, `secrets.env`,
session databases, or machine-specific paths.

## Pull requests

- Keep changes focused. Prefer a harness fix over a model-specific hack.
- Add or update tests under `tests/` when behavior changes.
- Do not put secrets, home-directory paths, or personal emails in the tree.
- Match the existing Python 3.12 / Typer / FastAPI style.

## Reporting bugs

Open a GitHub issue with OS, Python version, `shadow doctor` output
(redact secrets), and the steps to reproduce.

For vulnerabilities, see [SECURITY.md](SECURITY.md).
