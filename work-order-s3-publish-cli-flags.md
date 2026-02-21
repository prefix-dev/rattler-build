# Work Order: Add S3 CLI credential flags to `publish` command

## Problem

The `publish` command (`rattler-build publish --to s3://...`) currently only supports
S3 credentials via:

1. A config file (`~/.rattler/config.toml` with `[s3-options.<bucket>]`)
2. The `rattler auth` keychain
3. AWS SDK default credential chain (env vars `AWS_ACCESS_KEY_ID`, etc.)

The legacy `upload s3` command supported **direct CLI flags** for S3 credentials:

```
--endpoint-url <URL>       (env: S3_ENDPOINT_URL)
--region <REGION>           (env: S3_REGION)
--access-key-id <KEY>       (env: S3_ACCESS_KEY_ID)
--secret-access-key <KEY>   (env: S3_SECRET_ACCESS_KEY)
--session-token <TOKEN>     (env: S3_SESSION_TOKEN)
--addressing-style <STYLE>  (env: S3_ADDRESSING_STYLE)
```

These flags (defined in `rattler_s3::clap::S3CredentialsOpts`) are useful for:
- CI/CD pipelines that pass credentials as arguments or `S3_*` env vars
- One-off uploads without needing to configure a config file first
- Users migrating from `upload s3` to `publish`

Since `upload` is being superseded by `publish`, users lose these CLI flags with
no equivalent replacement.

## Scope of Changes

All changes are in **rattler-build** only (no upstream crate changes needed).

### Files to modify

#### 1. `src/opt.rs` — Add S3 credential CLI flags to `PublishOpts`

Add `S3CredentialsOpts` (from `rattler_s3::clap`) as a flattened field on `PublishOpts`,
gated behind `#[cfg(feature = "s3")]`:

```rust
// In PublishOpts struct:
#[cfg(feature = "s3")]
#[clap(flatten)]
pub s3_credentials: rattler_s3::clap::S3CredentialsOpts,
```

This adds the following CLI flags to `rattler-build publish`:
- `--endpoint-url` / `S3_ENDPOINT_URL`
- `--region` / `S3_REGION`
- `--access-key-id` / `S3_ACCESS_KEY_ID`
- `--secret-access-key` / `S3_SECRET_ACCESS_KEY`
- `--session-token` / `S3_SESSION_TOKEN`
- `--addressing-style` / `S3_ADDRESSING_STYLE`

These flags should only take effect when `--to` is an `s3://` URL. When provided,
they should **override** the corresponding values from the config file.

Update `PublishData` and `PublishData::from_opts_and_config()` to carry the
parsed `Option<rattler_s3::S3Credentials>` through.

#### 2. `src/publish.rs` — Use CLI-provided S3 credentials in `upload_to_s3()`

In the `upload_to_s3()` function (line ~328), update the credential resolution to
merge CLI-provided credentials with config file values. Priority order:

1. **CLI flags** (`--endpoint-url`, `--access-key-id`, etc.) — highest priority
2. **Config file** (`[s3-options.<bucket>]` in `~/.rattler/config.toml`)
3. **Auth storage** (`rattler auth` keychain — for access key/secret)
4. **AWS SDK default chain** — lowest priority fallback

Concretely, if `PublishData` carries `Some(S3Credentials)` from CLI flags:
- Use those credentials directly (merged with auth storage for any missing access keys)
- Skip the config file lookup for this bucket

If CLI credentials are `None`, fall through to the existing config + auth + SDK flow.

#### 3. `src/tool_configuration.rs` — Minor adjustment to `resolve_s3_credentials()`

Add an optional `cli_credentials: Option<&rattler_s3::S3Credentials>` parameter to
`resolve_s3_credentials()` so that CLI-provided creds can be passed in and take
priority over the config file lookup.

Alternatively, the merging can happen entirely in `publish.rs` before calling
`resolve_s3_credentials()`, whichever is cleaner.

### Credential merge logic

```
let effective_credentials = match (cli_creds, config_creds) {
    // CLI flags provided → use them (auth storage fills in access keys if needed)
    (Some(cli), _) => {
        let mut creds = cli.clone();
        // If access keys weren't provided on CLI, try auth storage
        if creds.access_key_id.is_none() {
            if let Some(resolved) = creds.resolve(bucket_url, &auth_storage) {
                return Ok(resolved);
            }
        }
        creds.into_resolved()
    }
    // No CLI flags → fall back to config file (existing behavior)
    (None, Some(config)) => { /* existing code path */ }
    // Nothing → AWS SDK default chain
    (None, None) => rattler_s3::ResolvedS3Credentials::from_sdk().await?
};
```

### What NOT to change

- The `upload` subcommand — it already has these flags via `rattler_upload::UploadOpts`
- The `rattler_s3` or `rattler_upload` crates — no upstream changes needed
- The config file format — it remains a valid (but lower-priority) source
- The `CommonOpts` / `BuildOpts` structs — S3 flags are only relevant for `publish`

## Testing

1. **Manual test**: `rattler-build publish pkg.conda --to s3://bucket/channel --endpoint-url http://localhost:9000 --region us-east-1 --access-key-id minioadmin --secret-access-key minioadmin --addressing-style path`
2. **Env var test**: Set `S3_ENDPOINT_URL`, `S3_REGION`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` and verify they're picked up
3. **Priority test**: Set config file values AND CLI flags, verify CLI wins
4. **Backward compat**: Verify `publish --to s3://...` still works with config-file-only setup (no CLI flags)
5. **Non-S3 test**: Verify `publish --to https://prefix.dev/...` still works and S3 flags are ignored

## Migration path for users

**Before (upload s3):**
```bash
rattler-build upload s3 pkg.conda \
  --channel s3://my-bucket/my-channel \
  --endpoint-url https://s3.custom.com \
  --region us-west-2 \
  --access-key-id AKIA... \
  --secret-access-key ...
```

**After (publish):**
```bash
rattler-build publish pkg.conda \
  --to s3://my-bucket/my-channel \
  --endpoint-url https://s3.custom.com \
  --region us-west-2 \
  --access-key-id AKIA... \
  --secret-access-key ...
```

The flags are identical — only the subcommand and `--channel`→`--to` change.
