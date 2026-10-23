#!/bin/sh
# Local development runner. Only provider API keys are read; env files are not executed.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$#" -gt 1 ]; then echo "Usage: $0 [env-file]" >&2; exit 2; fi
env_file=${1:-"$root/.env.local"}
if [ -f "$env_file" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            OPENROUTER_API_KEY=*|ANTHROPIC_API_KEY=*)
                value=${line#*=}
                case "$value" in
                    \"*\") value=${value#\"}; value=${value%\"} ;;
                    \'*\') value=${value#\'}; value=${value%\'} ;;
                esac
                case "$line" in
                    OPENROUTER_API_KEY=*)
                        if [ -z "${OPENROUTER_API_KEY:-}" ]; then export OPENROUTER_API_KEY="$value"; fi ;;
                    ANTHROPIC_API_KEY=*)
                        if [ -z "${ANTHROPIC_API_KEY:-}" ]; then export ANTHROPIC_API_KEY="$value"; fi ;;
                esac ;;
        esac
    done < "$env_file"
fi
cd "$root"
exec cargo run
