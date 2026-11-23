# Çoklu UYAP katmanı (rafta)

19.11.2026 kullanıcı kararı: "İşletim sistemi başlar başlamaz Rust kodu çalışmasın. Önce işletim sisteminin ana
çatısını kuralım; Çoklu UYAP'ı sonra ekleriz."

Bu klasördeki dosyalar eski **kiosk** düzenine aittir (açılışta `uyap_app`) ve `iso_derle.sh` tarafından imaja
**kopyalanmaz**:

| Dosya | Eski yeri | Not |
|---|---|---|
| `etc/systemd/system/armonia-kiosk.service` | `config/includes.chroot/...` | tty1'de labwc + uyap_app |
| `etc/systemd/system/armonia-kiosk.service.d/10-sandbox.conf` | `sertlestirme/...` | uyap_app kum havuzu (systemd) |
| `etc/apparmor.d/opt.armonia.cokluuyap.uyap_app` | `sertlestirme/...` | AppArmor profili (complain) |
| `opt/armonia/bin/kiosk_oturum.sh` | `config/includes.chroot/opt/armonia/bin/oturum.sh` | uyap_app yeniden başlatma döngüsü |
| `hooks/normal/0200-cokluuyap-postgresql.hook.chroot` | `0100-armonia.hook.chroot` içindeki "Faz 2 PostgreSQL" bloğu | Debian kümesini sil + doğrula, servisleri maskele, her cihazda aynı **snakeoil anahtarını sil** (denetçi O2, O3, D1). 23.11'de geri kondu. |
| `package-lists/cokluuyap.list.chroot` | `config/package-lists/armonia.list.chroot` | `postgresql` |

Kasa (LUKS2 + PostgreSQL) katmanı da Çoklu UYAP içindi: `../kasa/` (imaja girmiyor).

## Geri bağlanırken (plan: `docs/claude-notlar/planlar/armonia-herkes-icin.md`)
- `uyap_app` açılışta değil, **Ayarlar'dan** açılır ("Çoklu UYAP kullanıyorum").
- Kiosk birimi yerine kullanıcı oturumunda ayrı, kum havuzlu bir birim; sonsuz yeniden başlatma yok.
- `10-sandbox.conf` ve AppArmor profili yeni birime uyarlanır; yol `/opt/armonia/cokluuyap/uyap_app` aynı kalabilir.
- Tarayıcı politikasına `127.0.0.1:8800`, nft `tarayici_yerel` kurallarına 8800 eklenir.
- `hooks/normal/*` → `config/hooks/normal/`, `package-lists/*` → `config/package-lists/` kopyalanır. PostgreSQL
  kancası `0100-armonia`'dan sonra çalışır; soket erişimi verilecek kullanıcı `COKLUUYAP_KULLANICI` ile seçilir.
  `armonia` masaüstü kullanıcısı kum havuzunda olmadığı için **postgres grubuna eklenmemeli** (bütün oturum sokete erişir).
