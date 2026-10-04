#!/bin/bash
# Собирает движок AmneziaWG (amneziawg-go, лицензия MIT) из исходников на закреплённом коммите.
# Использование: scripts/build-amneziawg-go.sh <GOOS> <GOARCH> <куда положить бинарник>
#
# Один патч: на Android нет /var/run, поэтому каталог сокета управления берётся из переменной окружения
# WG_SOCKET_DIR (приложение передаёт свою закрытую папку). Остальной код — как в апстриме.
set -euo pipefail

AWG_COMMIT="b5928efb6ca19f0153958460c3d141f04abc5c2e"
GOOS_TARGET="${1:?GOOS}"
GOARCH_TARGET="${2:?GOARCH}"
OUTPUT="$(realpath -m "${3:?путь вывода}")"

WORK="$(mktemp -d)"
git init -q "$WORK"
cd "$WORK"
git remote add origin https://github.com/amnezia-vpn/amneziawg-go.git
git fetch -q --depth 1 origin "$AWG_COMMIT"
git checkout -q FETCH_HEAD
echo "amneziawg-go: $(git rev-parse HEAD)"

if [ -f ipc/uapi_unix.go ]; then
python3 - <<'PY'
p = "ipc/uapi_unix.go"
s = open(p).read()
old = 'var socketDirectory = "/var/run/amneziawg"'
assert old in s, "не нашли socketDirectory — апстрим изменился"
s = s.replace(
    old,
    'var socketDirectory = func() string {\n\tif dir := os.Getenv("WG_SOCKET_DIR"); dir != "" {\n\t\treturn dir\n\t}\n\treturn "/var/run/amneziawg"\n}()',
    1,
)
open(p, "w").write(s)
PY
fi

# Текст лицензии (MIT) нужно поставлять вместе с бинарником.
if [ -n "${LICENSE_OUT:-}" ]; then
  mkdir -p "$(dirname "$LICENSE_OUT")"
  cp LICENSE "$LICENSE_OUT"
fi

CGO_ENABLED=0 GOOS="$GOOS_TARGET" GOARCH="$GOARCH_TARGET" go build -trimpath -ldflags "-s -w" -o "$OUTPUT" .
ls -la "$OUTPUT"
