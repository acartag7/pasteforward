CARGO ?= cargo
BIN := target/release/pasteforward

.PHONY: build test integration fmt lint supply-chain verify clean install package

build:
	$(CARGO) build --locked --release

test:
	$(CARGO) test --locked

fmt:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --locked --all-targets -- -D warnings

integration: build
	sh scripts/test-cli-boundaries.sh

supply-chain:
	sh scripts/check-supply-chain.sh

verify: fmt lint test integration supply-chain

install: build
	install -m 0755 $(BIN) $(HOME)/.local/bin/pasteforward

package: verify
	scripts/package-release.sh

clean:
	$(CARGO) clean
