CARGO := $(HOME)/.cargo/bin/cargo

.PHONY: build check test format inspect patch
build:
	$(CARGO) build --workspace
check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace
test:
	$(CARGO) test --workspace
format:
	$(CARGO) fmt --all
inspect:
	$(CARGO) run -p gc-inspect -- "$(DISC)"
patch:
	python3 tools/build_20xx.py
