#!/usr/bin/env bash
# Smoke-tests a running Orbi's local server: auth, origin/host guards, /event,
# the /hello proof handshake, and /ask long-poll (answered by whatever you do — hotkey, tray — or timeout).
#   scripts/smoke.sh            # security + event checks
#   scripts/smoke.sh ask        # also fire one /ask and print the decision
set -u
DIR="${ORBI_DATA_DIR:-$HOME/Library/Application Support/Orbi}"
PORT=$(cat "$DIR/port" 2>/dev/null || echo 47821)
TOKEN=$(cat "$DIR/token")
BASE="http://127.0.0.1:$PORT"
pass=0; fail=0
check() { # name expected actual
  if [ "$2" = "$3" ]; then echo "  ok   $1"; pass=$((pass+1)); else echo "  FAIL $1 (want $2, got $3)"; fail=$((fail+1)); fi
}
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

echo "Orbi at $BASE"
check "health with token"      200 "$(code -H "Authorization: Bearer $TOKEN" $BASE/health)"
check "no token → 401"         401 "$(code $BASE/health)"
case "$TOKEN" in *0) BAD="${TOKEN%?}1";; *) BAD="${TOKEN%?}0";; esac
check "wrong token → 401"      401 "$(code -H "Authorization: Bearer $BAD" $BASE/health)"
check "browser Origin → 403"   403 "$(code -H "Authorization: Bearer $TOKEN" -H 'Origin: http://evil.test' $BASE/health)"
check "foreign Host → 403"     403 "$(code -H "Authorization: Bearer $TOKEN" -H 'Host: evil.test' $BASE/health)"
check "bad json → 400"         400 "$(code -X POST -H "Authorization: Bearer $TOKEN" -d '{nope' $BASE/event)"
check "oversize body → 413"    413 "$(head -c 300000 /dev/zero | tr '\0' 'a' | curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" --data-binary @- $BASE/event)"
check "event → 204"            204 "$(code -X POST -H "Authorization: Bearer $TOKEN" -d '{"agent":"claude-code","kind":"working","summary":"running `npm test`"}' $BASE/event)"
NONCE=$(openssl rand -hex 16)
WANT=$(printf 'hello\n%s\n%s' "$PORT" "$NONCE" | openssl dgst -sha256 -hmac "$TOKEN" | awk '{print $NF}')
GOT=$(curl -s -D - -o /dev/null -H "X-Orbi-Nonce: $NONCE" $BASE/hello | tr -d '\r' | awk -F': ' 'tolower($1)=="x-orbi-proof"{print $2}')
check "/hello proves the token" "$WANT" "$GOT"
BODY=$(curl -s -D "${TMPDIR:-/tmp}/orbi-h" -H "Authorization: Bearer $TOKEN" -H "X-Orbi-Nonce: $NONCE" $BASE/health)
WANT=$(printf 'resp\n%s\n%s' "$NONCE" "$BODY" | openssl dgst -sha256 -hmac "$TOKEN" | awk '{print $NF}')
GOT=$(tr -d '\r' < "${TMPDIR:-/tmp}/orbi-h" | awk -F': ' 'tolower($1)=="x-orbi-proof"{print $2}')
check "responses are signed" "$WANT" "$GOT"
check "unknown path → 404"     404 "$(code -H "Authorization: Bearer $TOKEN" $BASE/nope)"
check "not reachable off-loopback" 000 "$(code --max-time 1 http://$(ipconfig getifaddr en0 2>/dev/null || echo 10.255.255.1):$PORT/health)"

if [ "${1:-}" = ask ]; then
  echo "  … /ask pending — press ⌃⌥A / ⌃⌥D, or wait for the timeout"
  start=$(date +%s)
  out=$(curl -s -X POST -H "Authorization: Bearer $TOKEN" \
    -d "{\"agent\":\"claude-code\",\"tool\":\"Bash\",\"input\":{\"command\":\"rm -rf dist\"},\"cwd\":\"$PWD\"}" $BASE/ask)
  echo "  /ask → $out after $(( $(date +%s) - start ))s"
fi
echo "$pass passed, $fail failed"
[ $fail -eq 0 ]
