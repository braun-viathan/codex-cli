# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Compaction

Codex chooses between local and remote history compaction automatically by
default. Set `compact_mode` when you need to override that selection:

```toml
compact_mode = "local" # "auto", "local", or "remote"
experimental_compact_prompt_file = "/absolute/path/to/compact-prompt.md"
```

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
