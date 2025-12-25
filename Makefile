export RUST_LOG=info
export ACCESS_CONTROL_ALLOW_ORIGIN=*

server:
	cargo run --bin server -- start

client:
	SSH_USER=$(USER) \
		cargo run --bin daemon -- start

daemon: client

web:
	npx http-server -c-1 -p 8083 channel

build:
	cargo build --all

prod:
	cargo build --all --release

arm:
	cross build --all --target aarch64-unknown-linux-musl --release

format:
	cargo fmt --all

db:
	docker run --rm -it --name phirepass-valkey -p 6379:6379 valkey/valkey:9

docker-server:
	docker buildx build \
		-t dimitrmok/phirepass-server:latest \
		--platform linux/amd64,linux/arm64 \
		-f server/Dockerfile \
		--progress=plain \
		--push \
		.

docker-daemon:
	docker buildx build \
		-t dimitrmok/phirepass-daemon:latest \
		--platform linux/amd64,linux/arm64 \
		-f daemon/Dockerfile \
		--progress=plain \
		--push \
		.

docker-sandbox:
	docker buildx build \
        -t dimitrmok/phirepass-daemon-sandbox:0.0.4 \
        --platform linux/amd64 \
        -f daemon/Dockerfile.sandbox \
        --progress=plain \
        --push \
        .

wasm-dev:
	cd channel && \
        RUST_LOG=info wasm-pack build --target web \
            --out-name phirepass-channel \
            --out-dir pkg/debug

wasm-prod:
	cd channel && \
        RUST_LOG=info wasm-pack build --target web \
            --out-name phirepass-channel \
            --out-dir pkg/release \
            --release

wasm: wasm-dev wasm-prod

.PHONY: sftp
sftp:
	cd daemon && \
        RUST_LOG=info \
        PORT=12222 \
            cargo run --bin sftp-server

.PHONY: server deamon client web build arm format db docker-server docker-daemon wasm-dev wasm-prod
