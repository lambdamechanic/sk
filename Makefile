## Common dev tasks

.PHONY: precommit fmt clippy

precommit: fmt clippy

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

