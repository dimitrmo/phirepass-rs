export RUST_LOG=info
export RUST_BACKTRACE=full

server:
	cargo run --bin server -- start

client:
	SSH_PORT=12222 \
		cargo run --bin agent -- start

agent: client

web: wasm-dev
	npx http-server -c-1 -p 8083 channel

build:
	cargo build --all --all-features

prod:
	cargo build --all --release --all-features

arm:
	cross build --all --target aarch64-unknown-linux-musl --release

format:
	cargo fmt --all

db:
	docker run --rm -it --name phirepass-valkey -p 6379:6379 valkey/valkey:9

lint:
	cargo clippy --all --all-targets -- -D warnings -D dead_code

docker-server:
	docker buildx build \
		-t dimitrmok/phirepass-server:latest \
		--platform linux/amd64,linux/arm64 \
		-f server/Dockerfile \
		--progress=plain \
		--push \
		.

docker-agent:
	docker buildx build \
		-t dimitrmok/phirepass-agent:latest \
		--platform linux/amd64,linux/arm64 \
		-f agent/Dockerfile \
		--progress=plain \
		--push \
		.

docker-sandbox:
	docker buildx build \
        -t dimitrmok/phirepass-agent-sandbox:0.0.4 \
        --platform linux/amd64 \
        -f agent/Dockerfile.sandbox \
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

.PHONY: sshd
sshd:
	cd agent/sshd && \
    	docker build -t sshd-pass . && \
		docker run -it --rm -p 12222:22 \
            --name phirepass-sshd sshd-pass

.PHONY: server deamon client web build arm format db docker-server docker-agent wasm-dev wasm-prod
