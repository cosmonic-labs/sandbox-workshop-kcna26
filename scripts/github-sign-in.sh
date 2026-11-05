#!/usr/bin/env bash
# Sign the step 4 agent in to GitHub from a terminal, with GitHub's device
# flow (the "code flow"). For Codespaces, and for anyone whose browser isn't
# on the machine running the cluster. The chat page's browser flow is the
# main way in; this one shares GitHub's limit of 50 code sign-ins an hour for
# the whole OAuth app, so use it only when the browser flow can't work.
#
#   scripts/github-sign-in.sh          start, print the code, wait until signed in
#   scripts/github-sign-in.sh --out    sign out
#
# Each step is one curl you can run by hand (steps/04-github/README.md):
#   POST /github/sign-in    the code to enter, and where
#   GET  /github/status     pending, signed-in (with the login) or error
#   POST /github/sign-out   forget the sign-in
# None of them returns a token: the github-api plugin keeps it.
set -euo pipefail
source "$(dirname "$0")/env.sh"

URL="http://$HOST_HTTP"
call() { # method, path
  curl -s -m 30 -X "$1" -H 'Host: agent.localhost' "$URL$2"
}
field() { # name, json: one string or number field (the replies are flat JSON)
  sed -nE "s/.*\"$1\":(\"([^\"]*)\"|([0-9]+)).*/\2\3/p" <<<"$2"
}

if [ "${1:-}" = "--out" ]; then
  call POST /github/sign-out >/dev/null
  echo "Signed out of GitHub."
  exit 0
fi

reply=$(call POST /github/sign-in) || {
  echo "Can't reach the agent at $URL. Is step 4 applied? (kubectl apply -f steps/04-github/workload.yaml)" >&2
  exit 1
}
case "$(field state "$reply")" in
  pending) ;;
  *)
    echo "Sign-in didn't start: $(field error "$reply")" >&2
    exit 1 ;;
esac
echo "Open $(field verification_uri "$reply") and enter $(field user_code "$reply")"
echo "Waiting for GitHub (Ctrl-C to stop waiting; the code expires in $(( $(field expires_in "$reply") / 60 )) minutes)..."
while sleep 5; do
  reply=$(call GET /github/status || true)
  case "$(field state "$reply")" in
    pending) ;;
    signed-in)
      echo "Signed in to GitHub as $(field login "$reply")"
      exit 0 ;;
    signed-out)
      echo "Sign-in was cancelled." >&2
      exit 1 ;;
    *)
      echo "Sign-in ended: $(field error "$reply")" >&2
      exit 1 ;;
  esac
done
