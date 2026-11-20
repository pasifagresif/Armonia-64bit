#!/bin/sh
# Tum ekranlari kapat / ac (swayidle cagirir). wlr-randr Debian main'de. Claude, 2026-11-19.
case "$1" in kapat) d=--off ;; ac) d=--on ;; *) exit 2 ;; esac
for o in $(wlr-randr 2>/dev/null | awk '/^[^ ]/{print $1}'); do wlr-randr --output "$o" "$d"; done
