#!/usr/bin/env bash
# Starts the sync server's Docker image the way the self-hosting guide does and checks
# that it actually works as a container: the built-in health check passes, the
# non-root user can write to the /data volume, the admin token gates vault creation,
# `admin` works through `docker exec`, and data survives a container restart.
#
#   scripts/sync-server-smoke-test.sh [image]     (default: smaragd-sync-server)
#
# Run by CI after building the image; locally, `just docker-smoke` builds and runs it.
set -euo pipefail

IMAGE="${1:-smaragd-sync-server}"
NAME="smaragd-sync-smoke-$$"
VOLUME="$NAME-data"
TOKEN="smoke-test-token"
PORT="${SMOKE_PORT:-18080}"
BASE="http://127.0.0.1:$PORT/v1"

cleanup() {
    status=$?
    if [ "$status" -ne 0 ]; then
        echo "--- container logs ---"
        docker logs "$NAME" 2>&1 || true
    fi
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker volume rm "$VOLUME" >/dev/null 2>&1 || true
    exit "$status"
}
trap cleanup EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# The image's HEALTHCHECK runs every 30s; check more often here so the test is quick.
# It is still the image's own `smaragd-sync-server healthcheck` command being run.
start() {
    docker run -d --name "$NAME" \
        -p "127.0.0.1:$PORT:8080" \
        -v "$VOLUME:/data" \
        -e SMARAGD_SYNC_ADMIN_TOKEN="$TOKEN" \
        --health-interval=1s --health-start-period=30s \
        "$IMAGE" >/dev/null
}

wait_healthy() {
    for _ in $(seq 60); do
        case "$(docker inspect -f '{{.State.Health.Status}}' "$NAME")" in
            healthy) return 0 ;;
            unhealthy) fail "the container's health check reports unhealthy" ;;
        esac
        sleep 1
    done
    fail "the container did not become healthy within 60s"
}

status_of() {
    # Prints just the HTTP status code of a request.
    curl -s -o /dev/null -w '%{http_code}' "$@"
}

create_vault() {
    curl -s -o /dev/null -w '%{http_code}' -X POST "$BASE/vaults" \
        -H 'content-type: application/json' "$@" \
        -d '{"device_name":"smoke test","kdf_salt":"AAAAAAAAAAAAAAAAAAAAAA=="}'
}

echo "starting $IMAGE"
start
wait_healthy
echo "ok: healthy"

curl -sf "$BASE/health" | grep -q '"status":"ok"' || fail "GET /v1/health"
echo "ok: /v1/health"

[ "$(create_vault)" = 403 ] || fail "creating a vault without the admin token was not refused"
[ "$(create_vault -H 'x-admin-token: wrong')" = 403 ] || fail "a wrong admin token was accepted"
echo "ok: vault creation needs the admin token"

[ "$(create_vault -H "x-admin-token: $TOKEN")" = 201 ] \
    || fail "creating a vault with the admin token (is /data writable by the container user?)"
echo "ok: vault created, so /data is writable"

docker exec "$NAME" smaragd-sync-server admin list | grep -q '^1 vault(s)' \
    || fail "admin list through docker exec does not show the vault"
echo "ok: admin list via docker exec"

docker restart "$NAME" >/dev/null
wait_healthy
docker exec "$NAME" smaragd-sync-server admin list | grep -q '^1 vault(s)' \
    || fail "the vault did not survive a container restart"
echo "ok: data survives a restart"

[ "$(status_of "$BASE/vaults/00000000-0000-0000-0000-000000000000")" = 401 ] \
    || fail "an unauthenticated vault request was not refused"
echo "ok: vault endpoints need a device token"

echo "smoke test passed"
