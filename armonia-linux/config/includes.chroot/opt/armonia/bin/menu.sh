#!/bin/sh
# "A" menusu: kurulu uygulamalar (fuzzel). Tek ornek: ikinci basis menuyu kapatir. Claude, 2026-11-19.
pkill -x fuzzel && exit 0
exec fuzzel --config /etc/armonia/fuzzel.ini
