#!/usr/bin/env bash
set -euo pipefail

PAT=pat_LDCdDUdMfl1D.APfnvMMVZZSj560k-xkiXZ6PObDmt4OIY8dvetLk9JE
BASE_URL='http://localhost:8080'

# -----------------------------
# Config (override as needed)
# -----------------------------
BASE_URL="${BASE_URL:-http://localhost:8080}"
PAT="${PAT:-}"                       # required: pat_<token_id>.<secret>
NODE_HOSTNAME="${NODE_HOSTNAME:-dev-node}"
NODE_METADATA="${NODE_METADATA:-{\"env\":\"dev\"}}"

# -----------------------------
# Checks
# -----------------------------
if [[ -z "${PAT}" ]]; then
  echo "ERROR: PAT is required. Run with: PAT='pat_xxx.yyy' $0"
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "ERROR: curl is required"
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: python3 is required"
  exit 1
fi

echo "[1/8] Checking PyNaCl availability..."
python3 - <<'PY' >/dev/null 2>&1 || {
import nacl
PY
  echo "Installing pynacl..."
  python3 -m pip install --user pynacl >/dev/null
}

# -----------------------------
# Generate keypair
# -----------------------------
echo "[2/8] Generating Ed25519 keypair..."
readarray -t KEY_LINES < <(python3 - <<'PY'
import base64
from nacl.signing import SigningKey

sk = SigningKey.generate()
pk = sk.verify_key
b64u = lambda b: base64.urlsafe_b64encode(b).rstrip(b'=').decode()

print(b64u(bytes(sk)))
print(b64u(bytes(pk)))
PY
)
PRIVATE_KEY="${KEY_LINES[0]}"
PUBLIC_KEY="${KEY_LINES[1]}"

echo "Generated PUBLIC_KEY length: ${#PUBLIC_KEY}"

# -----------------------------
# Claim node (Step 1)
# -----------------------------
echo "[3/8] Claiming node via PAT bootstrap..."
CLAIM_RES="$(curl -sS -X POST "${BASE_URL}/api/nodes/claim" \
  -H "Authorization: Bearer ${PAT}" \
  -H "Content-Type: application/json" \
  -d "{
    \"public_key\":\"${PUBLIC_KEY}\",
    \"hostname\":\"${NODE_HOSTNAME}\",
    \"metadata\":${NODE_METADATA}
  }")"

echo "Claim response: ${CLAIM_RES}"

NODE_ID="$(python3 - <<PY
import json,sys
data=json.loads("""${CLAIM_RES}""")
nid=data.get("node_id")
ok=data.get("success") is True and isinstance(nid,str)
print(nid if ok else "")
PY
)"
if [[ -z "${NODE_ID}" ]]; then
  echo "ERROR: claim failed"
  exit 1
fi
echo "NODE_ID=${NODE_ID}"

# -----------------------------
# Challenge request (Step 2)
# -----------------------------
echo "[4/8] Requesting auth challenge..."
CHAL_RES="$(curl -sS -X POST "${BASE_URL}/api/nodes/auth/challenge" \
  -H "Content-Type: application/json" \
  -d "{\"node_id\":\"${NODE_ID}\"}")"

echo "Challenge response: ${CHAL_RES}"

CHALLENGE="$(python3 - <<PY
import json
data=json.loads("""${CHAL_RES}""")
print(data.get("challenge",""))
PY
)"
if [[ -z "${CHALLENGE}" ]]; then
  echo "ERROR: challenge request failed"
  exit 1
fi
echo "CHALLENGE=${CHALLENGE}"

# -----------------------------
# Sign challenge
# -----------------------------
echo "[5/8] Signing challenge..."
SIGNATURE="$(python3 - <<PY
import base64
from nacl.signing import SigningKey

priv="${PRIVATE_KEY}"
challenge="${CHALLENGE}"

pad=lambda s: s + "="*((4-len(s)%4)%4)
sk=SigningKey(base64.urlsafe_b64decode(pad(priv)))
sig=sk.sign(challenge.encode()).signature
print(base64.urlsafe_b64encode(sig).rstrip(b'=').decode())
PY
)"
echo "SIGNATURE length: ${#SIGNATURE}"

# -----------------------------
# Verify challenge -> JWT
# -----------------------------
echo "[6/8] Verifying challenge and requesting JWT..."
VERIFY_RES="$(curl -sS -X POST "${BASE_URL}/api/nodes/auth/verify" \
  -H "Content-Type: application/json" \
  -d "{
    \"node_id\":\"${NODE_ID}\",
    \"challenge\":\"${CHALLENGE}\",
    \"signature\":\"${SIGNATURE}\"
  }")"

echo "Verify response: ${VERIFY_RES}"

ACCESS_TOKEN="$(python3 - <<PY
import json
data=json.loads("""${VERIFY_RES}""")
print(data.get("access_token",""))
PY
)"
if [[ -z "${ACCESS_TOKEN}" ]]; then
  echo "ERROR: verify failed"
  exit 1
fi
echo "ACCESS_TOKEN obtained (len=${#ACCESS_TOKEN})"

# -----------------------------
# Negative test: replay same challenge
# -----------------------------
echo "[7/8] Replay test (should fail)..."
REPLAY_RES="$(curl -sS -X POST "${BASE_URL}/api/nodes/auth/verify" \
  -H "Content-Type: application/json" \
  -d "{
    \"node_id\":\"${NODE_ID}\",
    \"challenge\":\"${CHALLENGE}\",
    \"signature\":\"${SIGNATURE}\"
  }")"
echo "Replay response: ${REPLAY_RES}"

# -----------------------------
# Negative test: tampered signature
# -----------------------------
echo "[8/8] Tampered signature test (should fail)..."
TAMPERED_SIG="${SIGNATURE%?}A"
BAD_SIG_RES="$(curl -sS -X POST "${BASE_URL}/api/nodes/auth/verify" \
  -H "Content-Type: application/json" \
  -d "{
    \"node_id\":\"${NODE_ID}\",
    \"challenge\":\"${CHALLENGE}\",
    \"signature\":\"${TAMPERED_SIG}\"
  }")"
echo "Bad signature response: ${BAD_SIG_RES}"

echo
echo "Done."
echo "Summary:"
echo "  NODE_ID=${NODE_ID}"
echo "  ACCESS_TOKEN_LEN=${#ACCESS_TOKEN}"
echo "Expected negatives:"
echo "  replay verify => error"
echo "  tampered signature => error"
