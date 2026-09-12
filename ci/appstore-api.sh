# Sourced, never run. App Store Connect's API, for the two scripts that
# talk to it: local/make-ios-signing and ci/await-testflight-build.
#
# Before sourcing this, a caller must have set:
#
#   key_file       the .p8 the API key downloads as
#   work           a scratch directory to write into
#   response_file  where api() leaves what came back
#   die()          how this program gives up
#
# and APP_STORE_CONNECT_KEY_ID and APP_STORE_CONNECT_ISSUER_ID must be in
# the environment.

# shellcheck shell=bash
# Those four, and the token, are the caller's by that contract.
# shellcheck disable=SC2154

b64url() {
    base64 | tr -d '\n' | tr '+/' '-_' | tr -d '='
}

# Left-pad a hex string to the 32 bytes one half of an ES256 signature is.
pad32() {
    local hex
    hex="$(printf '%s' "$1" | sed 's/^0*//')"
    printf '%064s' "$hex" | tr ' ' 0
}

# The API takes a short-lived ES256 JWT rather than the key itself. JOSE
# wants the signature as raw r||s; openssl emits a DER SEQUENCE of two
# INTEGERs, either of which may carry a sign byte or be short. Hence
# unpacking it rather than base64-ing what openssl hands over.
#
# The token is good for ten minutes, so anything that waits longer than
# that has to mint another.
mint_token() {
    local now header payload signing_input der r s
    now="$(date +%s)"
    header="$(jq -nc --arg kid "$APP_STORE_CONNECT_KEY_ID" \
        '{alg: "ES256", kid: $kid, typ: "JWT"}')"
    payload="$(jq -nc --arg iss "$APP_STORE_CONNECT_ISSUER_ID" --argjson now "$now" \
        '{iss: $iss, iat: $now, exp: ($now + 600), aud: "appstoreconnect-v1"}')"

    signing_input="$(printf '%s' "$header" | b64url).$(printf '%s' "$payload" | b64url)"

    der="$work/signature.der"
    printf '%s' "$signing_input" | openssl dgst -sha256 -sign "$key_file" > "$der"

    r="$(openssl asn1parse -inform DER -in "$der" | awk -F: '/INTEGER/ {print $4}' | sed -n 1p)"
    s="$(openssl asn1parse -inform DER -in "$der" | awk -F: '/INTEGER/ {print $4}' | sed -n 2p)"

    printf '%s.%s' "$signing_input" \
        "$(printf '%s%s' "$(pad32 "$r")" "$(pad32 "$s")" | xxd -r -p | b64url)"
}

# Every call goes through here, so that a refusal is reported as the
# sentence Apple wrote rather than as a later jq finding no field in a
# body nobody looked at.
#
# The answer lands in a file rather than on stdout, so that this runs in
# the main shell. Called inside a command substitution instead, a failure
# here would exit only that subshell, and the caller would carry on with
# an empty string -- which is how a lookup that never happened turned
# into an attempt to register a bundle id that already existed.
api() {
    local method="$1" path="$2" body="${3:-}"
    local -a arguments=(
        --silent --show-error
        # Or curl reads the brackets in filter[identifier] as a range of
        # its own and refuses to make the request at all.
        --globoff
        --request "$method"
        --header "Authorization: Bearer $token"
        "https://api.appstoreconnect.apple.com$path"
    )
    if [ -n "$body" ]; then
        arguments+=(--header "Content-Type: application/json" --data "$body")
    fi

    : > "$response_file"
    curl "${arguments[@]}" > "$response_file" ||
        die "could not reach App Store Connect for $method $path."

    # A successful DELETE says nothing at all.
    [ -s "$response_file" ] || return 0

    jq -e . "$response_file" > /dev/null 2>&1 || {
        echo "$method $path did not answer with JSON:" >&2
        head -c 2000 "$response_file" >&2
        exit 1
    }

    if jq -e 'has("errors")' "$response_file" > /dev/null; then
        echo "App Store Connect refused $method $path:" >&2
        jq -r '.errors[] | "  \(.title): \(.detail)"' "$response_file" >&2
        exit 1
    fi
}
