CARGO ?= cargo
BIN := target/release/pasteforward

.PHONY: build test fmt lint supply-chain verify clean install package

build:
	$(CARGO) build --locked --release

test:
	$(CARGO) test --locked

fmt:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --locked --all-targets -- -D warnings

supply-chain:
	sh scripts/check-supply-chain.sh

verify: fmt lint test build supply-chain

install: build
	install -m 0755 $(BIN) $(HOME)/.local/bin/pasteforward

package: verify
	scripts/package-release.sh

clean:
	$(CARGO) clean
