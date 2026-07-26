# Container image for cull — https://github.com/rashida-thorne/cull
#
# The final image is `FROM scratch`: a single static musl binary, ~2 MB.
# TLS roots (webpki-roots) are compiled in, so no CA bundle is needed.
#
# Build (from a released version):
#   docker build --build-arg CULL_VERSION=0.6.0 -t cull .
# Use:
#   docker run --rm -i ghcr.io/rashida-thorne/cull '.title' -t < page.html
#   docker run --rm ghcr.io/rashida-thorne/cull 'h2 a' -a href https://example.com

FROM alpine:3.22 AS fetch
ARG TARGETARCH
ARG CULL_VERSION
RUN apk add --no-cache curl
RUN set -eu; \
    v="${CULL_VERSION#v}"; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported arch: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/rashida-thorne/cull/releases/download/v${v}/cull-v${v}-${target}.tar.gz" \
      | tar -xz -C /tmp; \
    install -m 0755 "/tmp/cull-v${v}-${target}/cull" /cull; \
    /cull --version

FROM scratch
COPY --from=fetch /cull /cull
ENTRYPOINT ["/cull"]
