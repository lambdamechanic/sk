## Summary
- Refresh cached repos during `sk update` by querying the remote default branch every run and resetting `origin/HEAD`.
- Add deterministic helpers + regression test to ensure changing the remote default branch is detected and reflected in the cache.

## Testing
- `cargo test update_cache_only`
