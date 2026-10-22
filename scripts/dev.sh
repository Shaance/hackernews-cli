#!/bin/sh
# Local development runner. Only OPENROUTER_API_KEY is read; env files are not executed.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$#" -gt 1 ]; then echo "Usage: $0 [env-file]" >&2; exit 2; fi
env_file=${1:-"$root/.env.local"}
if [ -z "${OPENROUTER_API_KEY:-}" ] && [ -f "$env_file" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            OPENROUTER_API_KEY=*)
                value=${line#*=}
                case "$value" in
                    \"*\") value=${value#\"}; value=${value%\"} ;;
                    \'*\') value=${value#\'}; value=${value%\'} ;;
                esac
                export OPENROUTER_API_KEY="$value"
                break ;;
        esac
    done < "$env_file"
fi
if [ -d "$root/.tools/terminal-browser/bin" ]; then
    export PATH="$root/.tools/terminal-browser/bin:$PATH"
fi
cd "$root"
exec cargo run
