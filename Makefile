.PHONY: clean build validator local test example metadata docs release

clean:
	@rm -rf test-ledger

metadata:
	@mkdir -p target/deploy
	@solana program dump --url mainnet-beta metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s metadata.so && mv metadata.so target/deploy/metadata.so

build: metadata
	@cd program && cargo build-sbf

test: metadata
	@cd program && cargo test-sbf

example: build
	@cd example && cargo test-sbf

docs:
	cargo doc --workspace --no-deps --open

release:
ifndef VERSION
	$(error VERSION is not set. Usage: make release VERSION=0.1.6)
endif
	cargo release $(VERSION) --workspace --execute

validator:
	solana-test-validator \
	  --clone-upgradeable-program metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s \
	  --bpf-program tape9hFAE7jstfKB2QT1ovFNUZKKtDUyGZiGQpnBFdL \
	  target/deploy/tape.so \
	  --url https://api.mainnet-beta.solana.com

local: clean build validator
