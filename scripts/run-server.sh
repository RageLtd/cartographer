#!/bin/sh
# The command the plugin registers as its MCP server.
#
# It exists because the binary is downloaded by a SessionStart hook, and on
# the first session after install the server can be started before that hook
# has finished. A stdio server that fails its first connection is not started
# again, so the tools would be missing until the user reconnected by hand.
# This fetches the binary when it is not there, then becomes it.

BINARY="${HOME}/.cartographer/bin/cartographer"

if [ ! -x "${BINARY}" ]; then
  # Standard output is the MCP channel, so the installer's messages go to
  # standard error. Its failure is reported below, with the same check.
  sh "$(dirname "$0")/ensure-binary.sh" >&2
fi

if [ ! -x "${BINARY}" ]; then
  echo "cartographer is not installed at ${BINARY} and could not be downloaded" >&2
  exit 1
fi

exec "${BINARY}" "$@"
