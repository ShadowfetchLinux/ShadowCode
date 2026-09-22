# Parallel worktrees (cap 2)

For one goal, ShadowCode may prepare at most **two** worker jobs in separate
managed git worktrees, plus the lead task on the source checkout.

- Workers never share one dirty tree.
- After both finish, a verifier reports conflicts instead of silently merging.
- Non-git workspaces: the feature stays disabled with a clear message.
- Cap is 2 because a 16 GB GPU typically holds one local model.

APIs: `POST /api/parallel/prepare`, `POST /api/parallel/verify`,
`POST /api/parallel/cleanup`.
