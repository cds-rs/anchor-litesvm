#!/usr/bin/env bash
# Render every PlantUML source under book/src to a sibling .svg using a
# PlantUML server (default http://localhost:8080). The SVGs are committed, so
# the book itself builds without a server; re-run this whenever a .puml changes.
#
#   ./render-diagrams.sh
#   PLANTUML_SERVER=http://host:8080 ./render-diagrams.sh
#
# Run a server locally with:
#   docker run -d -p 8080:8080 plantuml/plantuml-server
set -euo pipefail

server="${PLANTUML_SERVER:-http://localhost:8080}"
here="$(cd "$(dirname "$0")" && pwd)"
src="$here/src"

if ! curl -s -o /dev/null --max-time 5 "$server/"; then
  echo "error: PlantUML server not reachable at $server" >&2
  echo "       start one (docker run -d -p 8080:8080 plantuml/plantuml-server)" >&2
  echo "       or set PLANTUML_SERVER=<url>" >&2
  exit 1
fi

count=0
while IFS= read -r -d '' puml; do
  svg="${puml%.puml}.svg"
  # PlantUML's "~h" prefix means the diagram text is hex-encoded (no deflate),
  # so no client-side compression library is needed.
  hex="$(xxd -p "$puml" | tr -d '\n')"
  code="$(curl -s -o "$svg" -w '%{http_code}' "$server/svg/~h$hex")"
  if [ "$code" != "200" ]; then
    echo "error: $puml -> HTTP $code" >&2
    exit 1
  fi
  echo "rendered ${svg#"$here"/}"
  count=$((count + 1))
done < <(find "$src" -name '*.puml' -print0)

echo "rendered $count diagram(s) via $server"
