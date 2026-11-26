---
name: armonia-imaj-dogrulama-dersleri
description: Armonia ISO düzeltmelerini ISO derlemeden doğrulama yöntemi ve 25-26.11 gecesi bulunan gerçek kök nedenler
metadata:
  type: feedback
---

"Düzelttim" demeden önce düzeltmeyi imajın kendi ikilileriyle DENE; kullanıcıya 19 dakikalık ISO derlemesini kör test ettirme.

**Why:** 25-26.11 gecesi dört "düzeltme" denenmeden bildirildi, hiçbiri çalışmadı, kullanıcı haklı olarak çok sinirlendi. Gerçek nedenlerin hepsi ancak deneyince çıktı:
- Armonia Yazı "Operation was closed": ikili `tauri/custom-protocol` özelliği olmadan derlenmişti, gömülü arayüz yerine devUrl'e (127.0.0.1:5180) bağlanıyordu. Kum havuzu, DNS, WebKit sandbox teorilerinin hepsi yanlıştı. `derle.sh` artık `--features armonia-yazi/custom-protocol` veriyor.
- Tema değişince pencere şeridi eski renkte: Debian 13 procps-ng 4.0.4'te `pkill -HUP` → `-H -U P` diye ayrışıp "invalid user name: P" veriyor, sinyal hiç gitmiyor. Sayıyla ver: `pkill --signal 1 -x labwc`.
- İmaja eski ikili girmesi: `cokluuyap_hazirla.sh` / `yazi_hazirla.sh` çıktı varsa yeniden derlemiyordu (artık kaynak yeniyse derliyor). Ayrıca derleme `/root/armonia-cikti`'ya yazar; VM `~/armonia_seri.iso` kopyasını açar — kopyalamadan test edilirse eski imaj test edilir. Test öncesi `ls -la` ile tarihe bak.
- labwc istemcinin küçültme isteğini destekliyor; ama Wayland'da küçük pencereye çerçeve gelmediği için eframe `vsync: true` iken update döngüsü duruyor (uyap_app Linux'ta `vsync: false`).

**How to apply:** ISO'yu root'suz aç ve içinde çalıştır: `7z e iso live/filesystem.squashfs` → `unsquashfs -no-xattrs -ignore-errors -d tam ...` → `unshare -rm` altında /proc,/dev,/sys rbind + `/tmp/.X11-unix/X0` dosyasını bağla (dizin 1777 ve bizim olmalı) → `chroot tam env WLR_BACKENDS=x11 WLR_RENDERER=pixman GDK_BACKEND=wayland dbus-run-session -- labwc -C /etc/armonia/labwc`. Pencere "wlroots - X11-1"; PIL `ImageGrab` + `wmctrl`/`xdotool` ile görüntü al ve tıkla, görüntüyü Read ile oku. Değişen dosyaları ağaca kopyalayıp dene. Betiklerde `pkill -f` kullanma (kendi kabuğunu öldürür). Ağaç 4.7 GB; iş bitince sil.

26.11 ekleri:
- labwc'de bir uygulama arkadayken kendi penceresini öne ALAMAZ (Tauri `set_focus` etkisiz; denendi). Öne getirmek için masaüstü tarafından `armonia-one-getir "<başlık>"` (armonia-uygulamalar/dock, wlr-foreign-toplevel) çağrılır.
- İç içe oturum testinde `wmctrl -a` + `xdotool type/key` kullanıcının o an yazdığı tuşları test penceresine düşürür (kullanıcı aynı bilgisayarda çalışıyor). Klavye göndermek yerine komutla/fareyle dene, ekran görüntüsünü kısa tut.
- Test için kullanıcının gerçek UDF'lerini açma; `armonia-yazi/arayuz/test/ornek_uret.ts` ile deneme belgesi üret.
- Unix soket yolu ~107 baytı geçemez: scratchpad yolu çok uzun, test soketini `$XDG_RUNTIME_DIR` altına koy.
