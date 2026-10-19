CARGO := $(HOME)/.cargo/bin/cargo

.PHONY: build check test format inspect patch web web-check serve deploy
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
web:
	tools/build-web.sh
web-check: web
	npm test
serve: web
	npm run dev -- --port 8787
deploy: web
	npm run deploy
