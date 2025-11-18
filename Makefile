## Common dev tasks

QLTY ?= qlty
QLTY_FLAGS ?= --all --summary
QLTY_SMELLS_FLAGS ?= --all

.PHONY: precommit fmt clippy qlty qlty-smells 

precommit: fmt clippy qlty qlty-smells

fmt:
	cargo fmt --all 

clippy:
	cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features -- -D warnings


qlty:
	$(QLTY) check $(QLTY_FLAGS)


qlty-smells:
	QLTY="$(QLTY)" scripts/qlty-smells.sh
