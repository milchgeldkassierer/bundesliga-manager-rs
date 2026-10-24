#!/usr/bin/env bash
# Startet den GhidraMCP-Headless-Server (Ghidra 12.2, JDK 25) für die MCP-Bridge.
# Pfade per Umgebungsvariablen überschreibbar.
set -euo pipefail
GHIDRA_HOME=${GHIDRA_HOME:-$HOME/Apps/ghidra_12.2_PUBLIC}
JAVA_HOME=${JAVA_HOME:-$HOME/Apps/jdk-25.0.5+8}
MCP_JAR=${MCP_JAR:-$HOME/Apps/dl/src-gmcp/build/libs/GhidraMCP-7.0.0.jar}
PORT=${GHIDRA_MCP_PORT:-8089}
CP="$MCP_JAR"
for d in Framework Features Processors; do
  for j in "$GHIDRA_HOME"/Ghidra/$d/*/lib/*.jar; do CP="$CP:$j"; done
done
export GHIDRA_MCP_ALLOW_SCRIPTS=${GHIDRA_MCP_ALLOW_SCRIPTS:-1}
exec "$JAVA_HOME/bin/java" -Xmx6g -XX:+UseG1GC -Dghidra.home="$GHIDRA_HOME" -cp "$CP" \
  com.xebyte.headless.GhidraMCPHeadlessServer --bind 127.0.0.1 --port "$PORT" "$@"
