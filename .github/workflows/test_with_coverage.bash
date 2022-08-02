#!/usr/bin/env bash

PROJ_DIR="${1}"
ORIG_DIR="$(pwd)"

function usage
{
    printf "%s\n" "Usage: get_coverage.bash <project dir>"
}

if [ -z "${PROJ_DIR}" ] ; then
    usage
    exit 1
fi

function on_exit
{
    if (($? == 0)) ; then
        rm -rf "${scratchpad}"
    fi

    cd "${ORIG_DIR}"
}
trap on_exit EXIT

cd "${PROJ_DIR}" || exit 2

scratchpad="$(mktemp -d)"
printf "%s\n" "Profiling artefacts in ${scratchpad}"

RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="${scratchpad}/profiler-%m.profraw" cargo test --tests || exit 3

prof_out_file="${scratchpad}/profiler.profdata"
rust-profdata merge -sparse ${scratchpad}/*.profraw -o "${prof_out_file}" || exit 4

rust-cov report \
    $( \
      for file in \
        $( \
          RUSTFLAGS="-C instrument-coverage" \
            cargo test --tests --no-run --message-format=json \
              | jq -r "select(.profile.test == true) | .filenames[]" \
              | grep -v dSYM - \
        ); \
      do \
        printf "%s %s " -object $file; \
      done \
    ) \
    --instr-profile="${prof_out_file}" --summary-only --ignore-filename-regex='/.cargo/registry' || exit 5
