## Summary
- Implements `sk remove`, enforcing clean installs unless `--force` and updating the lockfile/install tree atomically.
- Adds coverage for clean removal, refusal on dirty installs, and forced removal in `tests/remove.rs`.

## Testing
- cargo test
