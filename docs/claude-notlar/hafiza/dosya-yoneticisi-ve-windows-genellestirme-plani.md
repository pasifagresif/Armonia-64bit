---
name: dosya-yoneticisi-ve-windows-genellestirme-plani
description: Thunar'ı kendi egui dosya yöneticimizle değiştirme + Windows uygulama katmanını (İcramatik özel adı) genel çoklu-uygulama yapısına çevirme — onaylandı, kod henüz yazılmadı
metadata:
  type: project
---

Kullanıcı 2026-11-24'te ikisini de onayladı; henüz kod yazılmadı (versiyon kontrolü/birleştirme arasına girdi).

## A) Windows uygulama katmanı — tek uygulamadan ("İcramatik") genel çoklu uygulamaya
Kullanıcının asıl şikayeti: "İcramatik gibi Windows uygulamalarını çalıştırabilsin" dedi, ama mimari tek bir
programa ("İcramatik") özel kuruldu — masaüstü dosyasında `Name=İcramatik`, dock CSS'inde `#custom-icramatik`,
systemd servisinde "Icramatik" yorumları. `windows.conf` da tek `PAYLASIM`/`PROGRAM` alıyor.

**Onaylanan yön:** Genel çoklu uygulama.
- `windows.conf` (tek dosya) → `windows.apps.d/*.conf` (her uygulama kendi dosyası: `ID`, `AD`, `PAYLASIM`,
  `PROGRAM`, `SHA256`). Örnek dosyada "İcramatik" sadece **yorum/örnek**, varsayılan değer olmaz.
- `paylasim_bagla.sh` → systemd **template unit** (`armonia-paylasim@.service`, `%i`=ID), her uygulama kendi
  `/srv/paylasim/<id>` noktasına bağlanır.
- `windows_calistir.sh` → `$1` olarak ID alır, kendi mount noktası + kendi Wine önekini
  (`~/.local/share/armonia-wine/<id>`) kullanır.
- Yeni `windows_hazirla.sh` (derleme script'i, `cokluuyap_hazirla.sh`/`yazi_hazirla.sh` gibi `iso_derle.sh`'tan
  çağrılır): `windows.apps.d/` içindeki her tanımdan `.desktop`, dock modülü (`custom/windows-<id>`,
  `dock.jsonc` + `dock.css.sablon`'a ekler) ve servis adını **otomatik üretir**. Sabit "İcramatik" dock/desktop
  dosyaları (`armonia-icramatik.desktop`, tekil `armonia-paylasim.service`) kaldırılır.
- `windows/BENIOKU.md` güncellenir; "İcramatik" artık tek örnek, mimari adı değil.
- Mevcut güvenlik çerçevesi (bwrap + cage + SHA256 bütünlük + ağsız) değişmeden her uygulama için ayrı ayrı
  uygulanır — zayıflatılmaz.
- İlgili dosyalar: `armonia-linux/windows/` (tüm katman), `armonia-linux/iso_derle.sh` (satır ~95-108, dock
  enjeksiyon kısmı), `armonia-linux/config/includes.chroot/etc/armonia/waybar/dock.jsonc`,
  `armonia-linux/config/includes.chroot/etc/armonia/tema/dock.css.sablon`.

## B) Thunar'ın yerine: Armonia Dosyalar (yeni egui uygulaması)
Kullanıcı Thunar'ın ikon/görünümünü istemiyor; kendi temaya uygun arayüz istiyor.

**Onaylanan yön:** Ayarlar ve Hukuk Hesaplayıcı gibi Rust+egui (WebKitGTK değil — proje zaten bu ikisi için egui
kullanıyor, `armonia-uygulamalar/` workspace; dosya yöneticisi de aynı zincire takılır, yeni derleme script'i
gerekmez).
- `armonia-uygulamalar/dosyalar/` (yeni workspace üyesi) — kapsam "temel tarayıcı":
  gezinme, liste/ikon görünümü, çift tık ile aç (mime'a göre: evince, libreoffice, armonia-yazi, resim
  görüntüleyici), kopyala/taşı/sil/yeniden adlandır, USB birimleri (udisks2/gio üzerinden) listesi.
  Kapsam DIŞI (şimdilik): arşiv aç/oluştur, "birlikte aç" menüsü, küçük resim önizleme, arama, yer imleri,
  özellikler penceresi — bunlar istenirse sonraki aşama.
- `config/package-lists/armonia.list.chroot`'tan `thunar` çıkar (gvfs/udisks2 kalır — mount altyapısı,
  Thunar'ın kendisi değil; `thunarx-3` kaldırma satırı da (`0100-armonia.hook.chroot`) artık gereksiz olur).
- `dock.jsonc`'taki `custom/belgeler` → `armonia-dosyalar` çalıştırır (şu an `thunar` çalıştırıyor).
- `uygulamalari_hazirla.sh`'a üçüncü ikili olarak eklenir (`for b in armonia-ayarlar hukuk_hesap dosyalar`).

## Sıra / bağımlılık
İkisi birbirinden bağımsız, istenen sırada yapılabilir. A (Windows genelleştirme) daha küçük/hızlı; B (dosya
yöneticisi) daha büyük bir yeni uygulama. Kullanıcıyla "hangisinden başlayalım" henüz kesinleşmedi — bu notu
okuyan oturum sorup başlayabilir.

İlgili: [[armonia-linux-2-hata-derleme-plani]] (aynı gün, versiyon kontrolü/birleştirme notu).
