#!/usr/bin/env bash
# Append GNU C dialect after BoringSSL's -std=c11 so NDK swab.h `asm()` parses.
exec "$@" -std=gnu11
