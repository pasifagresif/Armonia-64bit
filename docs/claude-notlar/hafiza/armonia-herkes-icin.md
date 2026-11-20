---
name: armonia-herkes-icin
description: 19.11.2026 yön değişikliği — Armonia her hukukçunun kurabileceği temel masaüstü; açılışta Rust (uyap_app) çalışmaz, Çoklu UYAP en son Ayarlar'dan açılan katman
metadata:
  type: project
---

Armonia Linux artık kiosk değil: Çoklu UYAP üyesi olsun olmasın her hukukçu indirip kurup kullanabilmeli.
Kullanıcı: "İşletim sistemi başlar başlamaz Rust kodu çalışmasın. Önce işletim sisteminin ana çatısını kuralım;
Çoklu UYAP'ı sonra ekleriz."

**Why:** Kiosk'ta açılışta uyap_app çalışıyor, UYAP bağlantısı kurulamayınca sistem işe yaramıyordu (14.11 donanım
denemesi). Ürün Çoklu UYAP üyeliğine bağımlı olmamalı.

**How to apply:**
- Sıra: temel sistem (greetd + labwc masaüstü, dock, tarayıcı, e-imza, ofis) → USB takınca sor → Ayarlar →
  Calamares diske kurulum → en son Çoklu UYAP katmanı (Ayarlar'dan açılır, varsayılan kapalı).
- Kararlar: canlı USB + diske kurulum; masaüstü Faz 5 tasarımı (labwc, dock, "A" menü, eş yükselti arka plan);
  tarayıcı yalnız adli/resmî siteler (izin listesi) + e-imza 5975; sertleştirme bugünkü kadar sıkı; USB takınca sor.
- Masaüstü bileşenleri uyap_app'e (127.0.0.1:8800) BAĞLANMAZ.
- Plan/ilerleme: `docs/claude-notlar/planlar/armonia-herkes-icin.md`. Kiosk dosyaları `armonia-linux/cokluuyap-katmani/`.
- Bu not [[armonia-gece-durum]] ve [[gemini-agy-ajan]]'daki kiosk varsayımlarının yerini alır.

- 19.11 sürücü kararı: sürücü paketleri imaja konmaz; kurulumda tespit edilen donanımın sürücüsü seçilir, sonradan takılan aygıt için onayla indirilir (isenkram + armonia-surucu@).
