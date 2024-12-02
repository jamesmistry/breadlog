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

# Generate a plain text report for easy consumption in stdout
cargo llvm-cov || exit 3

# Generate formatted HTML report (it will be stored in target/llvm-cov/html
cargo llvm-cov report --html || exit 4
