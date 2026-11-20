#!/bin/sh
# Armonia masaustu oturumu: labwc + ust cubuk + dock. Coklu UYAP (uyap_app) BURADA BASLAMAZ; sonraki asamada
# Ayarlar'dan acilan ayri bir birim olacak. greetd cagirir. Claude, 2026-11-19.
export XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=labwc:wlroots XDG_SESSION_DESKTOP=labwc
# Klavye duzeni: /etc/armonia/labwc/environment (Ayarlar yazar; labwc --reconfigure ile hemen uygulanir)
export LANG=tr_TR.UTF-8
# Cizici wlroots tarafindan secilir (GPU varsa gles2, yoksa pixman). pixman zorlanmaz (14 Kasim USB denemesi).
export WLR_NO_HARDWARE_CURSORS=1
# GTK/Qt/LibreOffice Wayland'de yerel calissin; X11'e dusmesinler
export GDK_BACKEND=wayland QT_QPA_PLATFORM=wayland SAL_USE_VCLPLUGIN=gtk3 MOZ_ENABLE_WAYLAND=1
# guvenlik-denetci O-X1 (14 Kasim): labwc tembel Xwayland icin X0 soketini dinliyor; X sunucusu hic baslamasin
export WLR_XWAYLAND=/bin/false
unset DISPLAY
# Belgeler/Indirilenler klasorleri (Turkce adlarla, ilk giriste)
command -v xdg-user-dirs-update >/dev/null && xdg-user-dirs-update
# -C: ayarlar kullanicinin degistiremeyecegi /etc/armonia/labwc'den (autostart, rc.xml, menu.xml, environment)
exec /usr/bin/labwc -C /etc/armonia/labwc
