# Armonia Linux — 2 hata düzeltmesi, yeniden derleme ve test planı
(2026-11-24, uzun oturum sonrası devam notu)

## ÖNEMLİ — çoklu bilgisayar senkronizasyonu (2026-11-24, ikinci oturum)
Kullanıcı aynı anda **başka iki bilgisayarda** da armonia-linux ve ÇokluUYAP üzerinde çalışıyor; o makinelerdeki
değişiklikler henüz git'e girmemişti (commit edilmemiş). Kullanıcı oradaki son halleri `origin/main`'e push edecek;
bu bilgisayarda ise ekranda duran commit edilmemiş değişiklikler burada. Çakışmayı önlemek için bu bölüm bu
oturumda **bu çalışma kopyasında** değişen HER ŞEYİN kesin listesi — pull/merge sırasında karşılaştırma için.

### Bu bilgisayarda commit edilmemiş değişikliklerin tam listesi (bu not yazıldığı an)
1. `armonia-linux/config/hooks/normal/0040-armonia-donanim-yazilimi.hook.chroot`
   — `export INITRD=No` eklendi (firmware kurulumunda initramfs yeniden oluşturmayı atlar/hızlandırır içerik gereği).
   NOT: ilk halinde yanlışlıkla iki kez eklenmişti, bu oturumda tekilleştirildi (davranış değişmedi).
2. `armonia-linux/config/includes.chroot/opt/armonia/bin/eimza_durum.sh`
   — `:175F ` → `:1757 ` (port 5983 hex yerine doğrusu 5975 hex). Bkz. aşağıdaki "1) E-İmza" bölümü.
3. `armonia-linux/config/includes.chroot/opt/armonia/bin/olcum.sh`
   — aynı `:175F ` → `:1757 ` düzeltmesi, iki satırda (bekleme döngüsü + rapor satırı).
4. `armonia-linux/iso_derle.sh`
   — `ACILIS=` satırındaki üç `$( [ ... ] && echo '...')` ifadesine `|| true` eklendi (`set -e` altında koşul
     yanlışken `&&`'in sıfırdan farklı çıkmasından kaynaklanan olası "pty/erken çıkış" türü durma riskini giderir).
     Satır sayısı aynı, tek satır, üç yerde `|| true` eklentisi.
5. `armonia-linux/yazi-katmani/includes.chroot/opt/armonia/bin/yazi_calistir.sh`
   — `export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` satırı eklendi (XDG_RUNTIME_DIR export'undan önce).
     Bkz. aşağıdaki "2) Armonia Yazı" bölümü.

### İzlenmeyen (untracked) dosyalar
- `armonia-linux/build.log` — derleme çalıştırma çıktısı, ARTEFAKT, commit edilmeyecek (.gitignore'a eklendi).
- `docs/claude-notlar/hafiza/armonia-linux-2-hata-derleme-plani.md` — bu dosyanın kendisi (yeni not).

### Push/pull sırası (kullanıcı ile kararlaştırıldı)
1. Kullanıcı diğer bilgisayardaki **Linux** değişikliklerini push edecek (en riskli: muhtemelen bu listedeki
   dosyalarla çakışabilir — hepsi `armonia-linux/` altında, aynı dosyalara dokunmuş olma ihtimali var).
2. Kullanıcı ÇokluUYAP için ayrı bir push yapacak (yeni sürüm; `Uyap Haricen Giriş/ruyap/` altında — bu oturumun
   `armonia-linux/` değişiklikleriyle dosya çakışması beklenmiyor, entegrasyonu muhtemelen kolay).
3. Bu bilgisayarda bu notta listelenen 5 dosya + build.log hâlâ commit edilmemiş haldeyse, push'lar `git pull`
   ile çekilmeden önce ya commit edilmeli ya da `git stash` ile kenara alınmalı (aksi halde pull, bu dosyalarda
   çakışma varsa durabilir). Kullanıcı onay verene kadar commit/push YAPILMAYACAK (CLAUDE.md kuralı) — ama pull
   öncesi en azından `git stash` önerilecek, iş kaybolmasın.
4. ÇokluUYAP tarafında yeni sürüm geldiğinde `derle_uyap.sh` çıktısı (`uyap_app`) yeniden derlenmeli; iş
   bilgisayarında kullanıcı kendisi `exe` olarak derletecek (ayrı, bu depo dışı bir adım).

## Durum

ISO daha önce başarıyla derlendi, QEMU'da çalıştı (ağ + GPU hızlandırma çalışıyor,
gerçek UYAP/adalet.gov.tr sitelerine erişim doğrulandı). İki kök neden bulundu ve
kaynakta düzeltildi ama **henüz yeniden derlenip doğrulanmadı**:

### 1) E-İmza "Açılıyor" takılı kalması (ÇÖZÜLDÜ, test bekliyor)
- Kök neden: `eimza_durum.sh` ve `olcum.sh` içinde port hex'i yanlış yazılmış.
  `0x175F` = 5983 ondalık, gerçek port 5975 ondalık = `0x1757`.
- Düzeltme (uygulandı): `:175F ` → `:1757 ` (her iki dosyada).
- Doğrulama: host'ta chroot içinde gerçek `adalet-eimza-tray` servisi çalıştırılıp
  port 5975'i başarıyla açtığı zaten kanıtlandı (bkz. `eimza_test.sh` / `eimza_test.log`
  — iş dizini `/home/pasifagresif/.claude/jobs/053634b4/tmp/`, kalıcı değil).
  Yani servisin kendisi sorunsuz; sadece göstergedeki hex typo düzeltildi.

### 2) Armonia Yazı "Operation was cancelled" hatası (ÇÖZÜLDÜ, test bekliyor)
- Kök neden: `armonia-yazi@.service` içindeki `RestrictNamespaces=yes` WebKitGTK'nın
  kendi iç sandbox'ının (unshare/clone) çalışmasını engelliyor. Kodun içinde bu durumu
  önceden öngören bir yorum vardı (denetçi notu 24.11 O1).
- Düzeltme (uygulandı): `yazi_calistir.sh` içine
  `export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` satırı eklendi
  (`XDG_RUNTIME_DIR` export'undan hemen önce). Mantık: systemd zaten tüm süreci
  sandbox'lıyor, WebKit'in kendi iç namespace sandbox'ına ihtiyaç yok.

## Değişen dosyalar (git status'ta M olarak görünüyor, commit edilmedi)
- `config/hooks/normal/0040-armonia-donanim-yazilimi.hook.chroot` (INITRD=No eklendi, ayrı/önceki hata)
- `config/includes.chroot/opt/armonia/bin/eimza_durum.sh`
- `config/includes.chroot/opt/armonia/bin/olcum.sh`
- `iso_derle.sh` (ACILIS= satırlarına `|| true`, ayrı/önceki hata)
- `yazi-katmani/includes.chroot/opt/armonia/bin/yazi_calistir.sh`

## Henüz çözülmemiş / ertelenmiş konular (bu derlemede test edilecek ama ayrı konu)
- labwc pencere dekorasyonu tema değişince güncellenmiyor (üst bar/dock güncelleniyor,
  yeni açılan pencereler eski/yeşil renkte kalıyor) — kök neden bulunamadı, kod
  (`tema_yenile()` / `labwc --reconfigure`) teoride doğru görünüyor.
- cokluuyap.com VM içinden erişilemiyor — muhtemelen kasıtlı (whitelist/politika filtresi),
  host'tan erişilebildiği doğrulandı, bug değil gibi görünüyor.
- Dock'ta "Çoklu UYAP" ve "İcramatik" kutularının ikonu yok (diğer 5 kutunun var) —
  `dock.css.sablon` içinde bilerek "simgesi olmayan kutucuklar" diye yorumlanmış, yani
  kasıtlı ama görsel tutarsızlık yaratıyor ("kalitesiz" hissi muhtemelen buradan).
  Kullanıcıya iki seçenek sunuldu: (a) bu ikisine de ikon ekle, (b) hepsini ikonsuz yap.
  Henüz karar verilmedi.

## Test planı (yeniden derleme sonrası)
1. `iso_derle.sh` çalıştır (tahmini ~18 dk, `/root/armonia-iso` build dizini).
2. `lb build` exit code 0 mı, tüm doğrulama kontrolleri (UEFI, squashfs içerik vb.) geçiyor mu bak.
   NOT: Türkçe locale'de `find -ipath`/`-iname` İ/i harfinde bozuluyor, doğrulama
   komutlarını `LC_ALL=C` ile çalıştır.
3. QEMU'da aç (OVMF vars dosyasını `$HOME/armonia_ovmf_vars.fd`'ye kopyalayıp kullan,
   sudo'ya gerek yok — root-owned dosya izin hatası yaşanmıştı).
   Komut iskeleti (GPU + ağ açık):
   ```
   qemu-system-x86_64 -enable-kvm -cpu host -m 4096 \
     -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
     -drive if=pflash,format=raw,file=$HOME/armonia_ovmf_vars.fd \
     -drive file=/root/armonia-cikti/armonia.iso,media=cdrom \
     -device virtio-vga-gl -display gtk,gl=on \
     -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
     -usb -device usb-tablet
   ```
4. Doğrula:
   - Üst barda "E-İmza: Hazır" görünüyor mu (artık "Açılıyor"da takılı kalmamalı).
   - UYAP girişi / e-imza ile oturum açma gerçekten çalışıyor mu.
   - Armonia Yazı açılınca "Operation was cancelled" hatası artık çıkmıyor mu.
   - (Bonus) tema değişince pencere dekorasyonu güncelleniyor mu — hâlâ bozuksa not al, bu ayrı konu.

## Not
Commit/push kullanıcı açıkça isteyene kadar yapılmayacak (CLAUDE.md kuralı).
