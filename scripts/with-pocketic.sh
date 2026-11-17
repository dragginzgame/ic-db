#!/usr/bin/env bash
# One command-scoped server owner; native callers only borrow its endpoint.
set -euo pipefail
if [[ ${1:-} == --console ]]; then
  # Console and qualification use the launcher's maximum one-hour command budget.
  shift
fi
if [[ $# == 0 ]]; then
  echo 'Usage: with-pocketic.sh [--console] COMMAND [ARG ...]' >&2
  exit 2
fi
if [[ ${IC_TESTKIT_POCKET_IC_URL+x} ]]; then
  exec "$@"
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
launcher="$(bash "$root/vendor/toko-tooling/scripts/dev/install-rust-tools.sh" \
  --consumer "$root" --package ic-testkit --version 0.110.0 \
  --bin ic-testkit-server --profile release --features cli --check)" || {
  echo 'Prepare the reviewed server launcher with make m0-setup.' >&2
  exit 1
}
# No binary fallback or implicit setup; the released launcher admits the assets.
umask 077
mkdir -p "$root/qualification/target/evidence/testkit"
attempt="$(mktemp -d "$root/qualification/target/evidence/testkit/run.XXXXXX")"
exec env -u POCKET_IC_BIN "$launcher" run "$root/.tools/testkit-server" "$attempt/evidence" --ttl 3600 -- "$@"
