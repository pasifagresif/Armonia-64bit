-- Büro belgelerinin klasör düzeni (24.11.2026, plan docs/claude-notlar/planlar/yazi-cokluuyap.md "Büro Belgeleri").
-- Son sürüm <kök>/Belgeler/<Yargı türü>/<Birim>/<yıl-sıra>/<ad>.<uzantı> dosyasında; eski sürümler aynı klasörün
-- .surumler/<belge no>/<N>.<uzantı> dosyalarında (belge_disk.rs). `yol` kökten göreli, '/' ayraçlı; NULL = eski
-- içerik adresli depoda (ekler/<sha256>), açılışta bir kez klasörlere taşınır.
ALTER TABLE belge ADD COLUMN IF NOT EXISTS yol varchar(1000) NULL;
-- Aynı dosyaya iki belge yazılmasın (Windows büyük/küçük harf ayırmaz)
CREATE UNIQUE INDEX IF NOT EXISTS ux_belge_yol ON belge (lower(yol)) WHERE yol IS NOT NULL;
