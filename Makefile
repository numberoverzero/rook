.PHONY: release-musl debug-musl

release-musl:
	docker-build/build.sh
debug-musl:
	cargo build