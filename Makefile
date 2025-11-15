## Common dev tasks

QLTY ?= qlty
QLTY_FLAGS ?= --all --summary --no-upgrade-check
QLTY_SMELLS_FLAGS ?= --all --no-upgrade-check

.PHONY: precommit fmt clippy qlty qlty-advisory qlty-smells qlty-smells-advisory

precommit: fmt clippy qlty qlty-smells-advisory

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

qlty:
	$(QLTY) check $(QLTY_FLAGS)

qlty-advisory:
	@echo "Running qlty (advisory; failures won't stop precommit)..."
	@$(QLTY) check $(QLTY_FLAGS) --no-fail || { \
		status=$$?; \
		echo "qlty failed to run (exit $$status). See logs above for details."; \
		exit $$status; \
	}

qlty-smells:
	$(QLTY) smells $(QLTY_SMELLS_FLAGS)

qlty-smells-advisory:
	@echo "Running qlty smells (advisory; failures won't stop precommit)..."
	@$(QLTY) smells $(QLTY_SMELLS_FLAGS) || { \
		status=$$?; \
		echo "qlty smells failed to run (exit $$status). See logs above for details."; \
		exit $$status; \
	}
