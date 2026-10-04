# shellcheck shell=bash
# Builds the throwaway notebook that docs/demo/demo.tape records against.
#
# Sourced by the tape's shell (bash) inside the VHS container, where `tasq` is
# on PATH. Everything lives in a fresh temp directory: HOME is moved there so
# no real config or notebook is read, and TASQ_NOW pins the clock so the gif
# is reproducible: the tape runs on Monday 2026-10-05, so `tasq summary` with no
# argument reports Friday 2026-10-02, where most of the history below lands.

demo="$(mktemp -d)"
mkdir -p "$demo/nb/demo"
: > "$demo/nb/demo/.index"
export HOME="$demo" NB_DIR="$demo/nb" TASQ_NOTEBOOK=demo TASQ_BOOKKEEPER=native
# No `less` or `claude` in the container: print directly, summarize without an LLM.
export TASQ_PAGER=cat TASQ_SUMMARIZER=raw

TASQ_NOW="2026-10-01 09:12" tasq create "Review MR !4821: invoice export" \
    --prio A --status in-progress --tag gitlab \
    --mr "https://gitlab.example.com/acme/billing/-/merge_requests/4821" \
    --note "assigned as reviewer" >/dev/null
TASQ_NOW="2026-09-30 11:30" tasq create "Upgrade the CI runners to Debian 13" \
    --due 2026-10-09 --tag infra --note "runner image draft pushed" >/dev/null
TASQ_NOW="2026-10-01 14:05" tasq create "Flaky spec in BillingMailer" \
    --tag support --note "fails one run in ten on CI, never locally" >/dev/null
TASQ_NOW="2026-10-01 16:40" tasq create "Draft the Q4 roadmap" \
    --prio C --status later --note "waiting for the planning meeting" >/dev/null
TASQ_NOW="2026-10-02 10:00" tasq create "Staging database refresh" \
    --status waiting --tag infra --note "asked infra for a window" >/dev/null
TASQ_NOW="2026-10-02 15:20" tasq log 1 "first pass done, two questions on the CSV escaping" >/dev/null
TASQ_NOW="2026-10-02 17:10" tasq log 2 "image builds; pipeline green on the test branch" >/dev/null

export TASQ_NOW="2026-10-05 09:30"
