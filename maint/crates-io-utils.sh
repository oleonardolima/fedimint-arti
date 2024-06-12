# Utilities for querying crates.io

CRATES_IO_URL_BASE=https://crates.io/api

fail () {
    echo >&2 "$0: error: $*"
    exit 12
}

tmp_trap_exit_setup () {
    tmp=$(mktemp -d)
    exit_rc=8
    trap 'set +e; rm -rf "$tmp"; exit $exit_rc' 0
}

tmp_trap_exit_finish_ok () {
    exit_rc=0
}

# Queries
#   https://crates.io/api/$endpoint
# Expects to receive either
#   HTTP 200 and a json document which `jq "$expect_key"` accepts
#   HTTP 404 and a json document containing a `.error` key
# The fetched document is stored in "$output"
# The HTTP code is left in the global variable `http_code`
# (and also written to "$output.http")
crates_io_api_call () {
    local endpoint="$1"
    local expect_key="$2"
    local output="$3"

    local url="${CRATES_IO_URL_BASE}/$endpoint"

    curl -L -sS -o "$output" -w '%{http_code}' >"$output.http" "$url"
    http_code=$(cat "$output.http")

    case "$http_code" in
	200) expect="$expect_key" ;;
	404) expect=.errors ;;
	*)
	    cat -vet "$output" >&2
	    fail "unexpected HTTP response status code $http_code from $url"
	    ;;
    esac

    set +e
    jq -e "$expect" <"$output" >/dev/null
    jq_rc=$?
    set -e
    if [ $jq_rc != 0 ]; then
	cat -vet "$output" >&2
	fail "bad JSON data from $url (expected $expect)"
    fi
}
