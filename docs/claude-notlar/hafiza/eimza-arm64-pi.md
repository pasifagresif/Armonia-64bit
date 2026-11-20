---
name: eimza-arm64-pi
description: Adalet E-imza Pi'de (arm64) AKİS sürücüsü olmadan APDU yoluyla imzaladı (20.11.2026); gerekli JVM seçenekleri ve tuzaklar
metadata:
  type: project
---

20.11.2026: Raspberry Pi 5'te (arm64) avukat.uyap.gov.tr'de e-imza ile imza BAŞARILI. AKİS'in Linux arm64 sürücüsü
(libakisp11.so) yok (yalnız x64 .deb/.rpm; Mac ARM var), ama gerek kalmadı: ESYA, PKCS#11 yüklenemeyince saf Java
APDU yoluna (akiscif, javax.smartcardio → pcscd) geçiyor ve imzalıyor.

- Çalışan düzen: `~/adalet_app/baslat.sh` = arm64 JRE 21 + `eimza-tray-1.0.11.jar` + resmi Conveyor başlatıcısının
  seçenekleri. Şart olan: `--add-exports=jdk.crypto.cryptoki/sun.security.pkcs11.wrapper=ALL-UNNAMED` (yoksa APDU
  imzası IllegalAccessError). Tam liste resmi .deb'in `bin/adalet-eimza-tray` dizgelerinde.
- Tuzak 1: pcscd yeniden başlarsa Java'nın PC/SC bağlamı ölür (SCARD_E_NO_SERVICE) → e-imza uygulaması da yeniden
  başlatılmalı (Armonia birimine PartOf/BindsTo gibi bağ gerekir).
- Tuzak 2 (kök neden ÇÖZÜLMEDİ): kart bazen SCARD_W_UNRESPONSIVE_CARD'a düşüyor; yalnız kartı çıkar-tak düzeltiyor.
  Boşta bekleme (25 sn) ve kart probu tek başına tetiklemiyor. ACR39U, genel libccid ile; sistemde işe yaramaz
  libacsccid1:armhf (32 bit) var.
- Pi deneme imajında (pi_derle.sh) e-imza HENÜZ YOK; nftables tarayıcıyı 5975'ten engelliyor (kiosk dönemi kuralı).

İlgili: [[armonia-herkes-icin]], [[eimza-5975-teshis]]
